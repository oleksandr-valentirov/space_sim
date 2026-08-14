//! Апарат у грі справді відчуває повітря (ROADMAP K7c).
//!
//! K7b провів опір від ассета до межі й закінчив на тому, що `cd` можна
//! передати. Хто його передає — лишалося без відповіді, а поле, якого ніхто
//! не задає, це рівно той мертвий код, який K4b знайшов у K4: `field.c` умів
//! гармоніки, і ніхто їх не вмикав.
//!
//! Тому перевірка стоїть на рівні `/game`, а не `core-rs`: світ, годинник,
//! ланки, `Vessel::params` — увесь шлях, яким число з гри доходить до
//! інтегратора. Нижче він уже перевірений двічі (`core/test/test_prop.c` і
//! `core-rs/tests/ephemeris.rs`), і саме тому тут не потрібні ні допуски, ні
//! фізика: досить показати, що два світи, які різняться **лише** `cd`,
//! розходяться, а два з нульовим `cd` — ні.
//!
//! Демо-місія лишається без опору навмисно (`mission::world`), з тієї самої
//! причини, з якої вона лишилась без вітрила в K6b: halo-орбіта підбиралася
//! без нього, і додати силу туди означало б змінити зміст демонстрації під
//! приводом технічного кроку. На L2 повітря все одно рівно нуль.

use core_rs::{State, VesselParams};
use game::mission;
use game::world::World;

/// Земля в ассеті-фікстурі.
const EARTH: i32 = 3;

/// 220 км над середнім радіусом Землі — всередині смуги таблиці USSA-76, а не
/// на її межі (урок K7a: на межі модель розривна, і порівняння могло б впасти
/// на будь-який бік).
const ALTITUDE: f64 = 220.0e3;

/// Скільки летимо. Двадцять хвилин на такій висоті — це вже сотні метрів
/// розбіжності, тобто величина, яку не сплутаєш із шумом інтегратора при
/// допуску в сантиметр.
const FLIGHT_S: f64 = 1200.0;

fn blunt(cd: f64) -> VesselParams {
    VesselParams {
        mass_kg: 1000.0,
        area_m2: 20.0,
        cr: 0.0,
        cd,
    }
}

/// Світ з одним апаратом на низькій орбіті, з заданим `cd`.
///
/// Стан будується з ассета: положення Землі плюс радіус і колова швидкість,
/// нахилена так, щоб жодна складова не була нулем — вітер обертової
/// атмосфери має бути скісний до руху, інакше похибка могла б сховатися в
/// нулі (та сама причина, що в `core/scenario/sc_dragflight.c`).
fn low_orbit_world(cd: f64) -> World {
    // Курсор стартує там само, де апарат: епоха ассета — нуль часу для
    // ефемериди, а не для місії (`mission::world` робить так само).
    let t0 = 86_400.0;
    let mut world =
        World::new(&mission::default_asset(), mission::config(), t0, 1.0).expect("світ будується");

    let eph = world.ephemeris();
    let earth = eph.body_state(EARTH, t0).expect("Земля в межах ассета");

    // Середній радіус Землі в ассеті — 6371010 м (core/cook/cook_fixture.c).
    // Тут він потрібен лише щоб опинитися в повітрі, тож сотня метрів туди
    // чи сюди нічого не вирішує.
    let radius = 6_371_010.0 + ALTITUDE;
    let speed = (3.986_004_418e14_f64 / radius).sqrt();

    let mut start = State {
        r: earth.r,
        v: earth.v,
        t: t0,
    };
    start.r.x += radius;
    start.v.y += 0.8 * speed;
    start.v.z += 0.6 * speed;

    world.add_vessel("probe", start, t0 + FLIGHT_S, Some(blunt(cd)));
    world
}

fn flown(cd: f64) -> State {
    let mut world = low_orbit_world(cd);
    world.run_to_end(1.0, 64);
    world.vessels()[0].tip
}

/// Той самий апарат із `cd` і без нього приходить у різні місця.
#[test]
fn a_vessel_with_cd_flies_a_different_trajectory() {
    let with = flown(2.2);
    let without = flown(0.0);

    let dx = with.r.x - without.r.x;
    let dy = with.r.y - without.r.y;
    let dz = with.r.z - without.r.z;
    let moved = (dx * dx + dy * dy + dz * dz).sqrt();

    assert!(
        moved > 1.0,
        "опір мав зсунути апарат, а зсув {moved} м — це шум"
    );

    // Обидва світи дійшли до кінця місії, інакше різниця була б просто в
    // тому, що один порахував менше.
    assert_eq!(with.t, without.t, "порівнюються різні моменти часу");

    println!("{FLIGHT_S} с опору зсунули апарат на {moved:.4} м");
}

/// Два світи без опору — бітово однакові.
///
/// Контрольний дослід: без нього перший тест доводив би лише те, що два
/// прогони взагалі різні, а не те, що їх розрізняє саме `cd`.
#[test]
fn without_cd_the_two_worlds_agree_to_the_bit() {
    let a = flown(0.0);
    let b = flown(0.0);

    assert_eq!(a.r.x.to_bits(), b.r.x.to_bits());
    assert_eq!(a.r.y.to_bits(), b.r.y.to_bits());
    assert_eq!(a.r.z.to_bits(), b.r.z.to_bits());
    assert_eq!(a.v.x.to_bits(), b.v.x.to_bits());
    assert_eq!(a.v.y.to_bits(), b.v.y.to_bits());
    assert_eq!(a.v.z.to_bits(), b.v.z.to_bits());
}
