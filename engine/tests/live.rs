//! Живий прогноз проти закоміченого еталона (ROADMAP H5).
//!
//! Тепер, коли рушій рахує траєкторію сам, з'являється питання, якого до H5
//! не існувало: а чи то саме, що лежить у фікстурі? Відповідь не «так» і не
//! «ні», і саме тому її варто виміряти.
//!
//! Фікстура — розв'язок multiple shooting: сім ланок, кожна з власного вузла,
//! з розривами на швах (ROADMAP C4). Живий прогноз безперервний. На
//! halo-орбіті з множником 594 за оберт (C3) дві криві **зобов'язані**
//! розійтися; питання лише в тому, коли — і C4 уже дав очікування, вимірявши
//! «базову лінію без корекції» для цієї ж орбіти: 1.31 оберту.

use core_rs::{Ephemeris, PropConfig, Propagator, State};
use engine::live;
use engine::trajectory;
use std::sync::Arc;

const DAY: f64 = 86400.0;

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Скільки живий прогноз тримається еталона.
///
/// Порівняння в епохах фікстури, тож інтегрування йде ланками, що
/// приземляються на ці моменти. Для порівняння **траєкторій** це коректно;
/// послідовність кроків при цьому інша, ніж у безперервного прогону, і саме
/// тому вона тут не порівнюється (H1 міряв її окремо).
#[test]
fn the_live_prediction_tracks_the_reference_and_then_leaves_it() {
    let reference = trajectory::load();
    let eph = Arc::new(Ephemeris::load(&live::repo_asset()).expect("ассет"));

    let cfg = PropConfig {
        tol_m: 1e-2,
        h_max_s: 3600.0,
        ..PropConfig::default()
    };
    let mut prop = Propagator::new(eph, cfg).expect("пропагатор");

    let mut state = live::fixture_start();
    let mut step = 0.0;

    // Момент, коли розбіжність уперше перевищує поріг. Пороги — не критерій
    // приймання, а сітка, на якій видно ФОРМУ розходження: експонента чи ні.
    let mut first_over = [None::<f64>; 4];
    let thresholds = [1.0e3, 1.0e5, 1.0e7, 1.0e9];

    for sample in reference.iter().skip(1) {
        let run = prop
            .run(&state, sample.t, &[], &mut [], &mut step)
            .expect("прогін у межах ассета");
        state = run.final_state;

        let live_r = [state.r.x, state.r.y, state.r.z];
        let miss = distance(live_r, sample.vessel);

        for (i, limit) in thresholds.iter().enumerate() {
            if first_over[i].is_none() && miss > *limit {
                first_over[i] = Some(sample.t);
            }
        }
    }

    for (limit, when) in thresholds.iter().zip(first_over.iter()) {
        match when {
            Some(t) => println!("  розійшлися на {limit:e} м через {:.2} діб", t / DAY),
            None => println!("  розбіжність так і не перевищила {limit:e} м"),
        }
    }

    // Кілометр — це вже далеко за похибкою інтегрування (сантиметр допуску) і
    // ще незрівнянно менше за саму орбіту (радіус ~4·10⁸ м). Тобто перший
    // поріг ловить момент, коли починає працювати нестійкість, а не шум.
    let km = first_over[0].expect("кілометрову розбіжність мали побачити");
    assert!(
        km > 5.0 * DAY,
        "живий прогноз відірвався від еталона за {:.2} доби — це занадто \
         швидко для орбіти з множником 594 за оберт (14.5 доби)",
        km / DAY
    );

    // І розходження мусить бути видно: якщо криві не розійшлися за сто діб,
    // значить порівнюють не те, що думають.
    assert!(
        first_over[3].is_some(),
        "за 101 добу нестійка halo-орбіта мала піти від еталона далі, ніж на \
         10⁹ м — те, що не пішла, означає помилку порівняння"
    );

    // Головне тут — не «коли», а «як швидко»: чотири порядки між першим і
    // третім порогом дають темп зростання, і він мусить збігтися з тим, що
    // ROADMAP C3 вимахав з монодромної матриці цієї ж орбіти — 594 за оберт.
    //
    // Це і є перевірка, що розходяться саме через нестійкість орбіти, а не
    // через помилку в стані, одиницях чи епосі: така помилка дала б або
    // лінійне зростання, або зовсім інший темп.
    let revolution = 101.8 * DAY / 7.0;
    let decades = 4.0;
    let span = first_over[2].unwrap() - first_over[0].unwrap();
    let per_revolution = 10f64.powf(decades * revolution / span);
    println!(
        "  темп: ×{per_revolution:.0} за оберт ({:.2} доби), передбачення C3 — ×594",
        revolution / DAY
    );

    assert!(
        (100.0..10_000.0).contains(&per_revolution),
        "темп розходження ×{per_revolution:.0} за оберт не схожий на ×594 з \
         монодромної матриці — розходяться не через нестійкість"
    );
}

/// Лінія, порахована зараз, справді малюється — в обох фреймах.
///
/// Той самий клас перевірки, що `both_frames_draw_visible_pixels` для
/// фікстури (F6): порожній кадр і правильний виглядають однаково, тож
/// рахуються пікселі.
#[test]
fn the_live_trajectory_draws_in_both_frames() {
    use engine::gpu::Gpu;
    use engine::trajectory_render::{geocentric_framing, render, rotating_framing, Params};

    const SIZE: u32 = 256;

    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu");
        return;
    };

    let live = live::propagate(&live::fixture_start(), 14.0, &live::repo_asset()).expect("прогноз");

    assert!(
        live.legs > 1,
        "буфер мав розрізати прогноз на ланки, а вийшла одна"
    );
    assert!(live.samples.len() > 100, "замало семплів для лінії");

    for (rotating, framing) in [
        (false, geocentric_framing(&live.samples)),
        (true, rotating_framing(&live.samples)),
    ] {
        let shot = render(
            &gpu,
            SIZE,
            SIZE,
            &live.samples,
            &Params {
                rotating,
                framing,
                colour: [0.9, 0.6, 0.2, 1.0],
            },
        )
        .expect("рендер");

        let mut lit = 0u64;
        for y in 0..shot.height {
            for x in 0..shot.width {
                let p = shot.pixel(x, y);
                if p[0] > 5 || p[1] > 5 || p[2] > 5 {
                    lit += 1;
                }
            }
        }

        assert!(lit > 100, "фрейм rotating={rotating}: {lit} пікселів");
    }
}

/// Прогноз — це та сама траєкторія, що політ.
///
/// Дві ланки по буферу проти одного прогону на той самий час: бітово те саме
/// (CLAUDE.md, інваріант 5). H1 міряв це в C; тут — через увесь ланцюжок, з
/// того боку межі, де живе гра.
#[test]
fn a_prediction_and_a_flight_are_the_same_trajectory() {
    let eph = Arc::new(Ephemeris::load(&live::repo_asset()).expect("ассет"));
    let cfg = PropConfig {
        tol_m: 1e-2,
        h_max_s: 3600.0,
        ..PropConfig::default()
    };

    let start = live::fixture_start();
    let t_end = start.t + 3.0 * DAY;

    // «Політ»: без семплів, одним прогоном.
    let mut flight = Propagator::new(eph.clone(), cfg).expect("пропагатор");
    let mut flight_step = 0.0;
    let flown = flight
        .run(&start, t_end, &[], &mut [], &mut flight_step)
        .expect("політ");

    // «Прогноз»: ланками по буферу, як його рахуватиме планер.
    let mut predict = Propagator::new(eph, cfg).expect("пропагатор");
    let mut buffer = [State::default(); 16];
    let mut predict_step = 0.0;
    let mut state = start;
    let mut legs = 0;

    loop {
        let run = predict
            .run(&state, t_end, &[], &mut buffer, &mut predict_step)
            .expect("ланка прогнозу");
        state = run.final_state;
        legs += 1;

        if run.stop == core_rs::Stop::ReachedEnd {
            break;
        }
    }

    assert!(legs > 1, "прогноз мав розрізатися на ланки");
    assert_eq!(state.r.x.to_bits(), flown.final_state.r.x.to_bits());
    assert_eq!(state.r.y.to_bits(), flown.final_state.r.y.to_bits());
    assert_eq!(state.r.z.to_bits(), flown.final_state.r.z.to_bits());
    assert_eq!(state.v.x.to_bits(), flown.final_state.v.x.to_bits());
    assert_eq!(state.v.y.to_bits(), flown.final_state.v.y.to_bits());
    assert_eq!(state.v.z.to_bits(), flown.final_state.v.z.to_bits());
    assert_eq!(predict_step.to_bits(), flight_step.to_bits());
}
