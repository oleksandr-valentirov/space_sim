//! Фікстура флоту, якою міряється борг D7 (ROADMAP.md, N1).
//!
//! Вимір нею вартий рівно стільки, скільки варта сама фікстура, тож
//! перевіряється не «щось збудувалось», а три твердження, від яких залежать
//! числа N1:
//!
//! 1. **Апарати різні.** Тридцять копій однієї орбіти — це один апарат,
//!    поміряний тридцять разів; помилка такого роду не видно ні в часі кадру,
//!    ні в кількості вершин.
//! 2. **Орбіти справді колові й на заявленій висоті.** Густина семплів,
//!    заради якої флот існує, — властивість висоти; станція на витягнутому
//!    еліпсі дала б іншу й тихо.
//! 3. **Станції долітають.** Опір на низькій орбіті — не декорація: апарат,
//!    що зійшов з орбіти посеред виміру, зменшує його мовчки.

use game::mission;
use game::world::EARTH;

/// Скільки станцій перевіряти. Більше за сім (період таблиці площин) і
/// більше за чотири — щоб повторення, якщо воно є, встигло проявитись.
const STATIONS: usize = 12;

fn build() -> game::world::World {
    mission::fleet(&mission::default_asset(), STATIONS).expect("флот будується на фікстурі")
}

#[test]
fn every_station_flies_its_own_orbit() {
    let world = build();
    let eph = world.ephemeris();
    let start = mission::start();
    let earth = eph.body_state(EARTH, start.t).expect("Земля в ассеті є");

    // Halo лишається першим — на ньому стоїть решта гри.
    assert_eq!(world.vessels().len(), STATIONS + 1);
    assert_eq!(world.vessels()[0].name, "halo 1151");

    let mut directions: Vec<[f64; 3]> = Vec::new();
    let mut radii: Vec<f64> = Vec::new();
    for vessel in &world.vessels()[1..] {
        let r = [
            vessel.tip.r.x - earth.r.x,
            vessel.tip.r.y - earth.r.y,
            vessel.tip.r.z - earth.r.z,
        ];
        let radius = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        directions.push([r[0] / radius, r[1] / radius, r[2] / radius]);
        radii.push(radius);
    }

    // Оболонки різні — це те, що дає різну густину семплів.
    for (i, a) in radii.iter().enumerate() {
        for b in &radii[i + 1..] {
            assert!((a - b).abs() > 1.0e3, "дві станції на одній оболонці");
        }
    }

    // **І напрямки різні теж — окремим твердженням.** Різниці радіусів
    // достатньо, щоб «дві станції на одній орбіті» ніколи не спрацювало, тож
    // перевірка, що зупиняється на ній, не побачила б флоту, у якому таблиця
    // площин прочитана з одним і тим самим індексом. Саме такої фікстури тут
    // не має бути (D13, D14: симетрична фікстура ховає помилку тричі).
    let mut distinct = 0;
    for (i, a) in directions.iter().enumerate() {
        if !directions[..i]
            .iter()
            .any(|b| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0e-9))
        {
            distinct += 1;
        }
    }
    assert!(
        distinct >= 7,
        "лише {distinct} різних площин на {STATIONS} станцій — таблиця площин не читається"
    );
}

#[test]
fn every_station_starts_circular_at_its_shell() {
    let world = build();
    let eph = world.ephemeris();
    let start = mission::start();
    let earth = eph.body_state(EARTH, start.t).expect("Земля в ассеті є");
    let mu = eph.body_mu(EARTH);
    let surface = eph.body_radius(EARTH);

    for vessel in &world.vessels()[1..] {
        let r = [
            vessel.tip.r.x - earth.r.x,
            vessel.tip.r.y - earth.r.y,
            vessel.tip.r.z - earth.r.z,
        ];
        let v = [
            vessel.tip.v.x - earth.v.x,
            vessel.tip.v.y - earth.v.y,
            vessel.tip.v.z - earth.v.z,
        ];
        let radius = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let altitude = radius - surface;

        // Смуга з `mission`: 600 км плюс 25 км на апарат, 29 оболонок.
        assert!(
            (600.0e3..=1300.0e3).contains(&altitude),
            "{} стартує на висоті {altitude:.0} м",
            vessel.name
        );

        // Колова — це дві умови, і друга (перпендикулярність) ловить те, чого
        // перша не бачить: швидкість правильної величини вздовж радіуса дала б
        // падіння на Землю з тим самим модулем.
        let circular = (mu / radius).sqrt();
        assert!(
            (speed - circular).abs() < 1.0e-6,
            "{}: {speed} проти колової {circular}",
            vessel.name
        );

        let along_radius = (r[0] * v[0] + r[1] * v[1] + r[2] * v[2]) / (radius * speed);
        assert!(
            along_radius.abs() < 1.0e-12,
            "{}: швидкість не перпендикулярна до радіуса ({along_radius})",
            vessel.name
        );
    }
}

#[test]
fn the_fleet_survives_the_span_it_is_measured_over() {
    let mut world = build();
    let start = mission::start();

    // Десять діб, а не сто: у debug сто коштували б хвилини, а те, що ловить
    // цей тест, — вхід в атмосферу — на 600 км за десять діб уже проявилось би
    // помилкою кроку, якби висоту вибрали неправильно. Повний спан міряє зонд
    // (`--perf-probe 101 --stations 30`), і там відмова теж друкується.
    world.run_to_day(start.t + 10.0 * 86400.0, 1.0, 8);

    for vessel in world.vessels() {
        assert!(
            vessel.failed.is_none(),
            "{} не долетів: {:?}",
            vessel.name,
            vessel.failed
        );
    }

    // Флот, який не полетів, теж «не впав». Густина семплів — те, заради чого
    // фікстура існує, тож вона й перевіряється: нижче за сотню на добу
    // означало б, що станції не там, де їх задумали.
    let snapshot = world.snapshot();
    let samples: usize = snapshot.vessels.iter().map(|v| v.sample_count()).sum();
    let per_vessel_day = samples as f64 / (snapshot.vessels.len() as f64 * 10.0);
    assert!(
        per_vessel_day > 100.0,
        "густина семплів {per_vessel_day:.0} на апарат за добу — флот не там, де задумано"
    );
}
