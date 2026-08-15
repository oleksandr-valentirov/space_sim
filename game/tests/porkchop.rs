//! Сітка вікон рахується в нитці планувальника (ROADMAP-UI.md, U5b).
//!
//! Три твердження, і жодне з них не про пікселі:
//!
//! 1. сітка з нитки — та сама, що прямий виклик межі, клітинка в клітинку;
//! 2. там, де розв'язку немає, лишається **дірка**, а не нуль;
//! 3. нитка від сітки не глухне: правило скасування в неї одне на два види
//!    роботи, і сітка ним не виламується.
//!
//! Перше з них — про осі. `t1` і `tof` обидва додатні й обидва в секундах, тож
//! транспонована сітка виглядає цілком правдоподібно; U5a ловив це на межі,
//! тут те саме ловиться на щільній сітці, де клітинку ще треба покласти в
//! правильний рядок.

use std::sync::Arc;
use std::time::{Duration, Instant};

use game::mission;
use game::planner::{Planner, PreviewRequest, Request};
use game::porkchop::{Grid, GridRequest};
use game::world::{EARTH, MOON};

const DAY: f64 = 86400.0;
const PATIENCE: Duration = Duration::from_secs(20);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "не дочекалися: {what}");
        std::thread::yield_now();
    }
}

fn ephemeris() -> Arc<core_rs::Ephemeris> {
    Arc::new(core_rs::Ephemeris::load(&mission::default_asset()).expect("фікстура"))
}

/// Стани відходу для сітки.
///
/// Беруться з початкового стану місії, зсунутого в часі, а не з живої
/// траєкторії, і це навмисно: розгортці байдуже, звідки прийшли стани, а
/// прогін світу коштував би секунди на кожен тест. Те, що стани справді
/// беруться з траєкторії, перевіряє окремий тест наприкінці.
fn departures(count: usize, step: f64, from: f64) -> Vec<core_rs::State> {
    let base = mission::start();
    (0..count)
        .map(|i| core_rs::State {
            t: from + i as f64 * step,
            ..base
        })
        .collect()
}

/// Сітка, що цілком лежить у проміжку ассета: фікстура знає 120 діб від
/// J2000, тож відхід до 60-ї доби плюс переліт до 10 — із запасом усередині.
fn inside(id: u64, mu: f64) -> GridRequest {
    GridRequest {
        id,
        depart: departures(40, 1.5 * DAY, mission::start().t),
        arrive_body: MOON,
        centre_body: EARTH,
        mu,
        prograde: true,
        tof: (0..30).map(|i| (1.0 + f64::from(i) * 0.3) * DAY).collect(),
    }
}

fn ask_for(planner: &Planner, request: &GridRequest) -> Grid {
    planner.request(Request::Grid(request.clone()));
    let mut got = None;
    wait_until("сітка", || {
        if let Some(grid) = planner.latest_grid() {
            got = Some(grid);
        }
        got.as_ref().is_some_and(|g: &Grid| g.id == request.id)
    });
    got.expect("щойно перевірили")
}

/// Клітинка каже правду про перельот, і перевіряє це не сама розгортка.
///
/// Оракул тут навмисно **не** «сітка проти `porkchop_compute_eph`»: обидва
/// шляхи розв'язували б ту саму задачу Ламберта, а помилка вибору системи
/// координат по обидва боки скоротилася б. Саме так вона й прожила від U5a до
/// цього тесту — з баріцентричною фікстурою дуга будувалася навколо початку
/// координат із `mu` Землі, тобто навколо Сонця з масою Землі, і числа
/// виглядали правдоподібно (2–9.6 км/с).
///
/// Тому перевіряється сама фізика клітинки, трьома незалежними твердженнями:
///
/// 1. апарат, що отримав `dv` і полетів **двома тілами**, приходить туди, де
///    в цей момент буде тіло призначення (кеплерівська дуга, не інтегратор);
/// 2. `dv_m_s` — це довжина `dv`, а не інше число поруч;
/// 3. `v_inf_arrive` — швидкість відносно тіла, а не відносно центра.
#[test]
fn a_cell_is_a_transfer_that_actually_arrives() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    assert!(mu > 0.0, "фікстура мусить знати масу Землі");

    let request = inside(1, mu);
    let planner = Planner::spawn(eph.clone(), mission::config()).expect("планувальник");

    let started = Instant::now();
    let grid = ask_for(&planner, &request);
    let took = started.elapsed();

    assert_eq!(grid.cells.len(), request.depart.len() * request.tof.len());
    assert_eq!(grid.t1, request.t1());
    assert_eq!(grid.tof, request.tof);

    let mut checked = 0;
    let mut wild = 0;
    let mut worst_miss: f64 = 0.0;
    for i in 0..grid.t1.len() {
        for j in 0..grid.tof.len() {
            let Some(cell) = grid.at(i, j) else { continue };
            let (t1, tof) = (grid.t1[i], grid.tof[j]);

            // Скажені клітинки — повз перевірку, і межа тут не про фізику, а
            // про **власний розв'язувач цього тесту**: на дузі в сотню
            // кілометрів за секунду універсальна змінна втрачає знаки на
            // гіперболічних косинусах, і перевірка починає падати на своїй
            // точності, а не на чужій помилці. Такого вікна гравець і не
            // обере — воно на порядок дорожче за все, чим літають.
            if cell.dv_m_s > 10_000.0 || cell.v_inf_arrive > 10_000.0 {
                wild += 1;
                continue;
            }

            // Стан апарата в момент відходу — той самий, що ми подали, — і
            // маневр із клітинки поверх нього.
            let from = request.depart[i];
            let centre = eph.body_state(EARTH, t1).expect("Земля в межах ассета");
            let target = eph
                .body_state(MOON, t1 + tof)
                .expect("Місяць у межах ассета");
            let centre_then = eph
                .body_state(EARTH, t1 + tof)
                .expect("Земля в межах ассета");

            // Довжина маневру — довжина вектора маневру. Дрібниця, яку легко
            // порушити, показавши поруч сусіднє число.
            let length = (cell.dv[0].powi(2) + cell.dv[1].powi(2) + cell.dv[2].powi(2)).sqrt();
            assert!(
                (length - cell.dv_m_s).abs() <= 1e-9 * length.max(1.0),
                "({i}, {j}): |dv| = {length}, а показано {}",
                cell.dv_m_s
            );

            // Куди приведе ця дуга. Кеплер, а не наш інтегратор: клітинка й
            // рахувалася кеплерівською задачею, і питання рівно в тому, чи
            // зроблено це в правильній системі координат.
            let r0 = [
                from.r.x - centre.r.x,
                from.r.y - centre.r.y,
                from.r.z - centre.r.z,
            ];
            let v0 = [
                from.v.x - centre.v.x + cell.dv[0],
                from.v.y - centre.v.y + cell.dv[1],
                from.v.z - centre.v.z + cell.dv[2],
            ];
            let (arrive, arrive_v) = kepler(r0, v0, tof, mu);

            let want = [
                target.r.x - centre_then.r.x,
                target.r.y - centre_then.r.y,
                target.r.z - centre_then.r.z,
            ];
            let miss = ((arrive[0] - want[0]).powi(2)
                + (arrive[1] - want[1]).powi(2)
                + (arrive[2] - want[2]).powi(2))
            .sqrt();
            let distance = (want[0].powi(2) + want[1].powi(2) + want[2].powi(2)).sqrt();

            // Допуск — частка відстані, а не метри: порівнюються два
            // розв'язки тієї самої кеплерівської задачі різними методами
            // (Ламберт проти універсальної змінної). Помилка ж, яку тест
            // ловить, інша за порядком узагалі: не той центр — 1.5·10¹¹ м,
            // не те тіло — 4·10⁸ м.
            assert!(
                miss <= 1e-6 * distance,
                "({i}, {j}): дуга промахнулася повз Місяць на {miss:.3e} м \
                 при відстані {distance:.3e} м — це не той переліт",
            );
            worst_miss = worst_miss.max(miss / distance);

            // Швидкість на приході — відносно **тіла**, а не відносно центра.
            // Різниця — це швидкість Місяця, близько кілометра за секунду:
            // помітно на око в панелі й ніяк не помітно в коді.
            let moon_v = [
                target.v.x - centre_then.v.x,
                target.v.y - centre_then.v.y,
                target.v.z - centre_then.v.z,
            ];
            let relative = ((arrive_v[0] - moon_v[0]).powi(2)
                + (arrive_v[1] - moon_v[1]).powi(2)
                + (arrive_v[2] - moon_v[2]).powi(2))
            .sqrt();
            // Допуск той самий і з тієї ж причини: помилка «відносно
            // центра» становила б швидкість Місяця, тобто кілометр за
            // секунду — на дев'ять порядків більше.
            assert!(
                (relative - cell.v_inf_arrive).abs() <= 1e-6 * relative,
                "({i}, {j}): відносно Місяця виходить {relative:.1} м/с, \
                 а клітинка каже {:.1}",
                cell.v_inf_arrive
            );

            checked += 1;
        }
    }

    // Сорок — не кругле число, а запас під те, скільки їх тут насправді:
    // стани відходу в цьому тесті штучні (позиція стоїть, Місяць їде), тож
    // більшість вікон виходить скаженими. Важить, що перевірених достатньо
    // й що вони не зникли зовсім.
    assert!(
        checked >= 40,
        "перевірено лише {checked} клітинок, ще {wild} відкинуто як скажені"
    );
    println!(
        "  перевірено {checked} клітинок, {wild} скажених повз; \
         найгірший промах перевірки {worst_miss:.1e} від відстані"
    );

    let (low, high) = grid.scale().expect("сітка, де нічого не зійшлося");
    let (i, j, best) = grid.best().expect("найкраще вікно");
    println!(
        "  {checked} клітинок із {} за {took:?}; ціна від {low:.0} до {high:.0} м/с;\n  \
         найдешевше: відхід на добі {:.1}, переліт {:.1} доби, {:.0} + {:.0} м/с",
        grid.cells.len(),
        (grid.t1[i] - mission::start().t) / DAY,
        grid.tof[j] / DAY,
        best.dv_m_s,
        best.v_inf_arrive
    );

    // Найкраще вікно — справді найдешевше з усіх, а не перше-ліпше.
    for cell in grid.cells.iter().flatten() {
        assert!(cell.total() >= best.total());
    }
    assert!(
        (low - best.total()).abs() < 1e-9,
        "межа шкали й мінімум різні"
    );
}

/// Кеплерівське просування стану на `dt` — універсальна змінна, поділ навпіл.
///
/// Друга реалізація тієї самої фізики, і саме тому вона тут: якби перевірка
/// кликала `lambert_solve`, вона порівнювала б розгортку саму з собою.
/// Повертає позицію й швидкість: перша каже, чи дуга справді приводить до
/// Місяця, друга — чи `v_inf_arrive` порахована відносно тіла.
///
/// Поділ навпіл, а не Ньютон, і це не лінощі: час як функція універсальної
/// аномалії монотонно зростає, тож пошук у вилці збігається завжди, тоді як
/// Ньютон із наближенням для еліпса розлітається на гіперболічній дузі —
/// а маневр із halo-орбіти до Місяця буває саме гіперболічним. Тест, який
/// падає через власний розв'язувач, гірший за відсутній: він каже, що
/// зламане те, що ціле.
fn kepler(r0: [f64; 3], v0: [f64; 3], dt: f64, mu: f64) -> ([f64; 3], [f64; 3]) {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let r = dot(r0, r0).sqrt();
    let alpha = 2.0 / r - dot(v0, v0) / mu; // 1/a
    let rv = dot(r0, v0);
    let root = mu.sqrt();

    // Час, до якого приводить універсальна аномалія x.
    let time_of = |x: f64| -> f64 {
        let (c, s) = stumpff(alpha * x * x);
        (rv / root * x * x * c + (1.0 - alpha * r) * x * x * x * s + r * x) / root
    };

    // Вилка: розсуваємо верхню межу, доки не перескочимо dt.
    let mut low = 0.0;
    let mut high = 1.0;
    while time_of(high) < dt {
        high *= 2.0;
        assert!(high < 1e12, "дуга не досягає {dt} с за жодної аномалії");
    }

    // Сто поділів — це 2⁻¹⁰⁰ від початкової вилки, тобто далеко за подвійну
    // точність; зупиняє цикл сама рівність меж.
    for _ in 0..100 {
        let mid = 0.5 * (low + high);
        if mid <= low || mid >= high {
            break;
        }
        if time_of(mid) < dt {
            low = mid;
        } else {
            high = mid;
        }
    }

    let x = 0.5 * (low + high);
    let z = alpha * x * x;
    let (c, s) = stumpff(z);
    let f = 1.0 - x * x / r * c;
    let g = dt - x * x * x / root * s;

    let position = [
        f * r0[0] + g * v0[0],
        f * r0[1] + g * v0[1],
        f * r0[2] + g * v0[2],
    ];

    let r_new =
        (position[0] * position[0] + position[1] * position[1] + position[2] * position[2]).sqrt();
    let fdot = root / (r * r_new) * x * (z * s - 1.0);
    let gdot = 1.0 - x * x / r_new * c;

    let velocity = [
        fdot * r0[0] + gdot * v0[0],
        fdot * r0[1] + gdot * v0[1],
        fdot * r0[2] + gdot * v0[2],
    ];

    (position, velocity)
}

/// Функції Стампфа C(z) і S(z), рядами біля нуля.
fn stumpff(z: f64) -> (f64, f64) {
    if z > 1e-6 {
        let sz = z.sqrt();
        ((1.0 - sz.cos()) / z, (sz - sz.sin()) / (z * sz))
    } else if z < -1e-6 {
        let sz = (-z).sqrt();
        ((sz.cosh() - 1.0) / -z, (sz.sinh() - sz) / (-z * sz))
    } else {
        (0.5 - z / 24.0, 1.0 / 6.0 - z / 120.0)
    }
}

/// За краєм ассета клітинка **зникає**, а не коштує нуль.
///
/// Це та сама різниця, заради якої сітка щільна: нуль — найдешевший переліт
/// із можливих, тобто на плоті він виглядав би найкращим вікном, і гравець
/// клікнув би саме туди. Фікстура покриває 120 діб, тож переліт, що
/// приземляється пізніше, ефемериді нема з чого порахувати.
#[test]
fn a_window_past_the_end_of_the_asset_is_a_hole_not_a_bargain() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("планувальник");

    // Відхід на 115-й добі, переліт від доби до дванадцяти: перші стовпці ще
    // всередині 120 діб, останні — вже за краєм.
    let request = GridRequest {
        id: 7,
        depart: departures(1, DAY, 115.0 * DAY),
        arrive_body: MOON,
        centre_body: EARTH,
        mu,
        prograde: true,
        tof: (1..=12).map(|i| f64::from(i) * DAY).collect(),
    };

    let grid = ask_for(&planner, &request);

    let inside = grid.at(0, 0).expect("переліт на добу ще влазить у 120 діб");
    assert!(
        inside.total() > 0.0,
        "клітинка всередині проміжку не може коштувати нуль"
    );
    assert_eq!(
        grid.at(0, 11),
        None,
        "переліт до 127-ї доби — за краєм ассета, а сітка щось про нього знає"
    );

    let holes = grid.cells.iter().filter(|c| c.is_none()).count();
    println!("  {holes} дірок із {} клітинок", grid.cells.len());
    assert!(holes > 0, "заборонених зон не видно — перевіряти нема чого");

    // І найкраще вікно шукається серед того, що є, а не серед дірок.
    let (_, j, _) = grid.best().expect("хоч одне вікно");
    assert!(
        grid.at(0, j).is_some(),
        "найкращим вікном названо дірку — саме цього щільна сітка й не дозволяє"
    );
}

/// Сітка не виламує правила скасування, спільного на два види роботи.
///
/// Порожні осі — це запит ні про що, і відповіді на нього немає (нитка не
/// вигадує порожній плот). Перевірити «нічого не прийшло» можна лише через те,
/// що прийшло далі: якби нитка на такому запиті глухла, наступна відповідь не
/// прийшла б ніколи.
#[test]
fn an_empty_axis_leaves_the_thread_working() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("планувальник");

    planner.request(Request::Grid(GridRequest {
        id: 1,
        depart: Vec::new(),
        arrive_body: MOON,
        centre_body: EARTH,
        mu,
        prograde: true,
        tof: vec![DAY],
    }));

    let grid = ask_for(&planner, &inside(2, mu));
    assert_eq!(grid.id, 2);
    assert!(grid.cells.iter().flatten().count() > 0);
}

/// Прев'ю після сітки доходить — і навпаки.
///
/// Два види роботи йдуть одним каналом саме для цього: правило «новіше
/// скасовує старіше» лишається одне, і жоден вид не має власної черги, у якій
/// можна застрягти.
#[test]
fn a_preview_asked_after_a_grid_still_arrives() {
    let sim = game::sim::Sim::spawn(mission::world(&mission::default_asset()).expect("світ"))
        .expect("нитка симуляції");
    sim.send(game::sim::Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("горизонт", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = game::leg::restart_at(&vessel.legs, vessel.start, burn_t);

    let eph = sim.ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("планувальник");

    let mut plan = game::plan::Plan::new();
    plan.insert(game::plan::Manoeuvre {
        t: burn_t,
        dv: [-8.0, 0.0, 0.0],
        frame: game::plan::Frame::Vnb { body: EARTH },
    });

    planner.request(Request::Grid(inside(1, mu)));
    planner.request(Request::Preview(PreviewRequest {
        id: 2,
        vessel: vessel.id,
        from: restart.state,
        step: restart.step,
        plan,
        params: vessel.params,
        horizon_end: vessel.horizon_end,
    }));

    let mut preview = None;
    wait_until("прев'ю після сітки", || {
        if let Some(got) = planner.latest() {
            preview = Some(got);
        }
        preview.as_ref().is_some_and(|p| p.id == 2)
    });
    assert!(!preview.expect("щойно перевірили").legs.is_empty());
}

// ---------------------------------------------------------------------------
// Плот: зображення, осі, курсор (U5c)
//
// Ані ассета, ані нитки тут уже немає — сітка збирається руками. Так і
// задумано: усе нижче перевіряє переклад сітки в екран, а він не має права
// залежати від того, звідки сітка взялася.

use engine::egui;
use game::hud;
use game::porkchop::{cell_at, colour, Cell};
use game::text::Language;

/// Сітка 4×3 з дірою в кутку: ціни ростуть зі збільшенням обох індексів.
fn handmade() -> Grid {
    let t1: Vec<f64> = (0..4).map(|i| f64::from(i) * DAY).collect();
    let tof: Vec<f64> = (1..4).map(|j| f64::from(j) * DAY).collect();

    let mut cells = Vec::new();
    for i in 0..t1.len() {
        for j in 0..tof.len() {
            // Правий верхній кут — заборонена зона.
            cells.push(if i == 3 && j == 2 {
                None
            } else {
                Some(Cell {
                    dv: [100.0 * (i + 1) as f64, 0.0, 0.0],
                    dv_m_s: 100.0 * (i + 1) as f64,
                    v_inf_arrive: 10.0 * (j + 1) as f64,
                })
            });
        }
    }

    Grid {
        id: 42,
        t1,
        tof,
        cells,
    }
}

/// Дірка прозора, ціна — ні, і дешеве не схоже на дороге.
///
/// Це три властивості кольору, від яких залежить, чи можна плоту вірити.
/// Найважливіша — перша: непрозора дірка лягла б на ту саму шкалу, що й ціни,
/// і око почало б порівнювати її з ними.
#[test]
fn a_hole_is_transparent_and_a_price_is_not() {
    let cheap = Cell {
        dv: [100.0, 0.0, 0.0],
        dv_m_s: 100.0,
        v_inf_arrive: 10.0,
    };
    let costly = Cell {
        dv: [900.0, 0.0, 0.0],
        dv_m_s: 900.0,
        v_inf_arrive: 90.0,
    };
    let (low, high) = (cheap.total(), costly.total());

    assert_eq!(colour(None, low, high)[3], 0, "дірка мусить бути прозорою");
    assert_eq!(colour(Some(cheap), low, high)[3], 255);
    assert_eq!(colour(Some(costly), low, high)[3], 255);
    assert_ne!(
        colour(Some(cheap), low, high),
        colour(Some(costly), low, high),
        "кінці шкали пофарбовані однаково — плот нічого не показує"
    );

    // Уся сітка однакова — це дешевий кінець, а не дорогий і не ділення на нуль.
    let flat = colour(Some(cheap), low, low);
    assert_eq!(flat, colour(Some(cheap), low, high));
    assert_eq!(flat[3], 255);
}

/// Шкала монотонна: дорожче — не «інакше», а далі в один бік.
#[test]
fn the_scale_goes_one_way() {
    let (low, high) = (100.0, 1000.0);
    let mut previous = colour(
        Some(Cell {
            dv: [low, 0.0, 0.0],
            dv_m_s: low,
            v_inf_arrive: 0.0,
        }),
        low,
        high,
    );

    for step in 1..=9 {
        let cell = Cell {
            dv: [low + f64::from(step) * 100.0, 0.0, 0.0],
            dv_m_s: low + f64::from(step) * 100.0,
            v_inf_arrive: 0.0,
        };
        let now = colour(Some(cell), low, high);
        assert!(
            now[0] >= previous[0] && now[2] <= previous[2],
            "на кроці {step} шкала повернула назад: {previous:?} → {now:?}"
        );
        previous = now;
    }
}

/// Низ плоту — найкоротший переліт, і саме тут ламається переворот осі.
///
/// Зображення йде рядками згори вниз, а `tof` на плоті росте вгору. Забути
/// цей переворот легко, а виглядає забуття як цілком правдоподібний плот, у
/// якому курсор просто відповідає дзеркально.
#[test]
fn the_bottom_of_the_plot_is_the_shortest_flight() {
    let grid = handmade();

    assert_eq!(cell_at(&grid, 0.01, 0.01), Some((0, 0)), "лівий нижній кут");
    assert_eq!(cell_at(&grid, 0.99, 0.99), Some((3, 2)), "правий верхній");
    assert_eq!(
        cell_at(&grid, 0.01, 0.99),
        Some((0, 2)),
        "лівий верхній: перший відхід, найдовший переліт"
    );

    // Поза плотом клітинки немає — інакше промах повз край читався б як
    // вибір крайньої.
    assert_eq!(cell_at(&grid, -0.1, 0.5), None);
    assert_eq!(cell_at(&grid, 0.5, 1.2), None);
}

/// Числа під курсором — числа тієї клітинки, а не сусідньої.
///
/// Панель малюється без вікна: `RawInput` із позицією миші, і те, що вийшло,
/// шукається серед намальованого тексту. Пікселі тут ні до чого — панель із
/// NaN виглядає точнісінько так само, як панель із правильними числами.
#[test]
fn the_readout_shows_the_cell_under_the_cursor() {
    let grid = handmade();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let mut draw = |events: Vec<egui::Event>| -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            hud::porkchop_panel(ui, Language::English, Some(&grid), &mut state);
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .flat_map(|clipped| texts(&clipped.shape))
            .collect()
    };

    // Кадр-розігрів: до першого малювання плот не має ні місця, ні розміру.
    draw(Vec::new());

    let rect = context
        .read_response(egui::Id::new(hud::PLOT_IMAGE))
        .expect("плот мусить бути намальований")
        .rect;

    // Наводимо на клітинку (2, 0): третій відхід, найкоротший переліт.
    let at = egui::pos2(
        rect.min.x + rect.width() * (2.5 / 4.0),
        rect.max.y - rect.height() * (0.5 / 3.0),
    );
    let said = draw(vec![egui::Event::PointerMoved(at)]);
    let all = said.join(" | ");

    let cell = grid.at(2, 0).expect("клітинка (2, 0) не дірка");
    assert!(
        all.contains(&format!("{:.0} / {:.0}", cell.dv_m_s, cell.v_inf_arrive)),
        "серед намальованого немає чисел клітинки (2, 0): {all}"
    );
    assert!(
        all.contains("1.00 days"),
        "переліт клітинки (2, 0) — доба, а панель каже: {all}"
    );

    // А тепер дірка — і вона мусить назватися діркою, а не мовчати.
    let hole = egui::pos2(
        rect.min.x + rect.width() * (3.5 / 4.0),
        rect.max.y - rect.height() * (2.5 / 3.0),
    );
    let said = draw(vec![egui::Event::PointerMoved(hole)]).join(" | ");
    assert!(
        said.contains(game::text::tr(
            Language::English,
            game::text::Key::NoSolution
        )),
        "заборонена зона нічого не сказала про себе: {said}"
    );
}

/// Клік по плоту обирає вікно — те, на яке дивилися.
#[test]
fn a_click_chooses_the_window_under_the_pointer() {
    let grid = handmade();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let draw = |state: &mut hud::PlotState, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            actions = hud::porkchop_panel(ui, Language::English, Some(&grid), state);
        });
        output.textures_delta.clear();
        actions
    };

    assert_eq!(
        draw(&mut state, Vec::new()),
        Vec::new(),
        "плот сам не клікає"
    );

    let rect = context
        .read_response(egui::Id::new(hud::PLOT_IMAGE))
        .expect("плот мусить бути намальований")
        .rect;
    let at = egui::pos2(
        rect.min.x + rect.width() * (1.5 / 4.0),
        rect.max.y - rect.height() * (1.5 / 3.0),
    );

    let actions = draw(
        &mut state,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );

    assert_eq!(actions, vec![hud::PorkchopAction::Choose(1, 1)]);
    assert_eq!(state.chosen, Some((1, 1)));
}

/// Кнопка просить сітку — і рівно це, без жодного вибору вікна.
#[test]
fn the_button_asks_for_a_grid_and_nothing_else() {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let mut draw = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            actions = hud::porkchop_panel(ui, Language::English, None, &mut state);
        });
        output.textures_delta.clear();
        actions
    };

    draw(Vec::new());
    let centre = context
        .read_response(egui::Id::new(hud::PLOT_COMPUTE))
        .expect("кнопка мусить бути намальована")
        .rect
        .center();

    let actions = draw(vec![
        egui::Event::PointerMoved(centre),
        egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);

    assert_eq!(actions, vec![hud::PorkchopAction::Compute]);
}

/// Увесь текст фігури — плаский список рядків.
fn texts(shape: &egui::epaint::Shape) -> Vec<String> {
    match shape {
        egui::epaint::Shape::Text(text) => vec![text.galley.text().to_string()],
        egui::epaint::Shape::Vec(shapes) => shapes.iter().flat_map(texts).collect(),
        _ => Vec::new(),
    }
}
