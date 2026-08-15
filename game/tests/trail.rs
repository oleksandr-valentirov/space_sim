//! На чому тримається кеш прорідженого сліду (ROADMAP.md, N2b).
//!
//! Кеш живе з припущення, що **позиція семпла в кадрі не залежить від часу
//! кадру**. Для інерціального фрейму це очевидно, для обертового — ні: базис
//! там будується з лінії Земля-Місяць, а вона обертається. Але будується він з
//! лінії **самого семпла**, а з кадру бере лише сталий масштаб і `μ`.
//!
//! Якщо це припущення хибне, кеш віддає вчорашню картинку — і жоден тест на
//! кількість вершин цього не побачить. Тому воно перевіряється прямо.

use engine::orbit::Orbit;
use game::frame_view::ViewFrame;
use game::{mission, trail, view};

fn camera() -> engine::camera::Camera {
    Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera()
}

/// Історія апарата — за кольором, а не «найдовша ламана».
///
/// Найдовшою на п'ятій добі є прогноз, а на двадцятій — уже історія, тож
/// вибір за довжиною порівнював би дві різні лінії.
fn history(scene: &engine::scene::Scene) -> Vec<[f64; 3]> {
    scene
        .polylines
        .iter()
        .find(|line| line.colour == game::palette::HISTORY.scene())
        .map(|line| line.points.clone())
        .unwrap_or_default()
}

/// Історія, намальована пізніше, **бітово** продовжує ту, що була намальована
/// раніше.
///
/// Не «майже така сама»: точки проходять `f64` до кінця, і будь-яка залежність
/// від часу кадру зсунула б їх усі. Прогін іде в обертовому фреймі, бо саме
/// там припущення неочевидне — за п'ятнадцять діб лінія Земля-Місяць повертає
/// на 200°.
#[test]
fn the_rotating_frame_does_not_move_a_sample_that_already_happened() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    let start = mission::start().t;

    world.run_to_day(start + 5.0 * 86400.0, 1.0, 8);
    let early = view::build_in(&world.snapshot(), camera(), ViewFrame::Rotating);

    world.run_to_day(start + 20.0 * 86400.0, 1.0, 8);
    let late = view::build_in(&world.snapshot(), camera(), ViewFrame::Rotating);

    let early_trail = history(&early);
    let late_trail = history(&late);

    assert!(
        early_trail.len() >= 2 && late_trail.len() > early_trail.len(),
        "слід не виріс: {} → {}",
        early_trail.len(),
        late_trail.len()
    );
    for (index, point) in early_trail.iter().enumerate() {
        assert_eq!(
            *point, late_trail[index],
            "точка {index} поїхала: позиція семпла залежить від часу кадру, \
             тобто кеш N2b тримати не можна"
        );
    }
}

/// Кеш тримає рівно ті ланки, які кадр питав, і викидає решту.
///
/// Без викидання каскад після правки плану лишав би в кеші ланки, яких у світі
/// вже немає (J3), і пам'ять росла б рівно так, як росте борг D7.
#[test]
fn the_cache_holds_what_the_frame_asked_for_and_nothing_else() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.run_to_day(mission::start().t + 10.0 * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();

    let legs: usize = snapshot.vessels.iter().map(|v| v.legs.len()).sum();
    assert!(legs > 1, "для перевірки треба більше однієї ланки");

    let mut cache = trail::Cache::new();
    let mut thinning = view::Thinning {
        cache: &mut cache,
        height_px: 720,
    };

    view::build_thinned(&snapshot, camera(), &[], ViewFrame::Inertial, &mut thinning);
    assert_eq!(thinning.cache.len(), legs);

    // Другий кадр із тим самим снапшотом нічого не додає: якби ключ був
    // нестабільний, кількість подвоїлася б.
    view::build_thinned(&snapshot, camera(), &[], ViewFrame::Inertial, &mut thinning);
    assert_eq!(thinning.cache.len(), legs);

    // Кадр, у якому апаратів немає, лишає кеш порожнім. Порожній світ, а не
    // копія снапшоту з обрізаним списком: `WorldSnapshot` навмисно не
    // клонується, і обходити це в тесті означало б перевіряти обхід.
    let empty = game::world::World::new(
        &mission::default_asset(),
        mission::config(),
        mission::start().t,
        mission::DEFAULT_WARP,
    )
    .expect("порожній світ будується")
    .snapshot();
    view::build_thinned(&empty, camera(), &[], ViewFrame::Inertial, &mut thinning);
    assert!(
        thinning.cache.is_empty(),
        "у кеші лишилося {} ланок, яких кадр не питав",
        thinning.cache.len()
    );
}

/// Той самий снапшот двічі дає ту саму сцену — теплий кеш нічого не міняє.
///
/// Оракул кешу, який неможливо пройти випадково: другий кадр іде цілком з
/// кеша, і якби той віддавав щось інше, різниця була б бітовою.
#[test]
fn a_warm_cache_draws_exactly_what_a_cold_one_did() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.run_to_day(mission::start().t + 10.0 * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();

    let mut cache = trail::Cache::new();
    let mut thinning = view::Thinning {
        cache: &mut cache,
        height_px: 720,
    };

    let cold = view::build_thinned(&snapshot, camera(), &[], ViewFrame::Rotating, &mut thinning);
    let warm = view::build_thinned(&snapshot, camera(), &[], ViewFrame::Rotating, &mut thinning);

    assert_eq!(cold.polylines.len(), warm.polylines.len());
    for (a, b) in cold.polylines.iter().zip(&warm.polylines) {
        assert_eq!(a.points, b.points, "теплий кеш намалював інше");
    }
}
