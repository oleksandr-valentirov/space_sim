//! Фізика імпульсу проти зовнішніх задач (ROADMAP L4, борг D1).
//!
//! `plan.rs` перевіряє, що світ виконує план так само, як зшиті руками
//! виклики `prop_run`. Це перевірка **машинерії**, і вона нічого не каже про
//! те, чи має імпульс фізичний сенс: обидві сторони там роблять `v += Δv` за
//! однією формулою, і обидві помилилися б однаково.
//!
//! Борг D1 називав цю дірку одним твердженням. Розібравшись, у ній видно
//! **два різні твердження**, і одним тестом вони не покриваються.
//!
//! ## 1. Імпульс приводить апарат туди, куди обіцяно
//!
//! Оракул — зовнішня задача: Ламберт (тепер на межі, L3) дає початкове
//! наближення, `prop_run_stm` виправляє його в повній моделі сил, і апарат
//! мусить прилетіти в **позицію Місяця з ассета**. Ціль тут не вигадана й не
//! порахована тією ж машинерією, що перевіряється: це число з ефемериди.
//!
//! Маневр подається у `Frame::Inertial` навмисно — щоб ця перевірка нічого не
//! знала про базис VNB. Її предмет — застосування імпульсу й сегментний цикл
//! навколо нього.
//!
//! ## 2. VNB означає те, що написано
//!
//! А ось тут зовнішня задача з пункту 1 не годиться, і це головне, що
//! з'ясувалося при плануванні L4. Якщо перекласти інерціальний Δv у VNB тим
//! самим базисом, яким гра його потім розгортає назад, помилка в базисі
//! **скоротиться сама із собою** — переставлені `normal` і `outward`
//! пройшли б таку перевірку бездоганно.
//!
//! Тому оракул інший, підручниковий, і жодне з його тверджень не походить із
//! `dv_inertial`:
//!
//! - чисто **прямий** імпульс паралельний швидкості, отже `|v+Δv| = |v|+|Δv|`
//!   і площина орбіти не змінюється;
//! - чисто **нормальний** перпендикулярний до неї, отже `|v+Δv|² = |v|²+|Δv|²`,
//!   а площина — змінюється;
//! - чисто **назовні** лежить у площині орбіти й дивиться **від** тіла.
//!
//! Прямий і нормальний розрізняються тим, як росте швидкість — лінійно проти
//! квадратично, — а не тим, як їх порахували.

use std::sync::Arc;

use core_rs::{lambert_solve, Ephemeris, PropConfig, Propagator, State, Vec3d};
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::world::{VesselId, World, EARTH};

const DAY: f64 = 86400.0;
const MOON: i32 = 4;

/// GM Землі, `data/horizons/obj_earth.txt` — те саме число, що в
/// `core/bench/bench_field.c` і в C-тестах. Ассет його через межу не віддає
/// (`eph_body_mu` на межі немає), тож воно тут виписане, а не прочитане.
const MU_EARTH: f64 = 3.98600435436e14;

/// Низька навколоземна орбіта, колова.
const R_LEO: f64 = 6.678e6;

/// Коли палимо і коли прилітаємо. Три доби — типовий переліт до Місяця, і
/// саме на цьому масштабі двотільний Ламберт помиляється настільки, що
/// корекція має що виправляти. Старт за десять хвилин до запалення: маневр
/// мусить бути в майбутньому відносно того, з чого апарат почався, інакше
/// його нікуди застосовувати.
const T0: f64 = T_BURN - 600.0;
const T_BURN: f64 = 2.0 * DAY;
const T_ARRIVE: f64 = T_BURN + 3.0 * DAY;

fn sub(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn add(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn scale(a: Vec3d, k: f64) -> Vec3d {
    Vec3d {
        x: a.x * k,
        y: a.y * k,
        z: a.z * k,
    }
}

fn unit(a: Vec3d) -> Vec3d {
    scale(a, 1.0 / norm(a))
}

fn norm(a: Vec3d) -> f64 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

fn dot(a: Vec3d, b: Vec3d) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn config() -> PropConfig {
    PropConfig {
        tol_m: mission::TOL_M,
        h_max_s: mission::H_MAX_S,
        ..PropConfig::default()
    }
}

/// Кругова орбіта відльоту **в площині перельоту**, і це не косметика.
///
/// Перша версія тесту брала готові числа з `core-sys/oracle.c` — і Ламберт
/// зажадав 11.7 км/с замість реалістичних чотирьох з половиною, бо площина
/// тієї орбіти не містила цілі, тобто половина Δv ішла на поворот площини.
/// Ньютон на такій задачі не збігався взагалі. Так само й будують реальні
/// місії: спершу площина, потім вікно.
///
/// Точка старту — приблизно навпроти цілі (~170°), щоб переліт був близький
/// до гомановського, а не до дуги в чверть оберту.
fn leo_start(eph: &Ephemeris, target_dir: Vec3d) -> State {
    let earth = eph.body_state(EARTH, T0).expect("Земля в межах ассета");

    let sideways = unit(cross(
        target_dir,
        Vec3d {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    ));
    let r_dir = unit(add(scale(target_dir, -1.0), scale(sideways, 0.18)));
    let plane_normal = unit(cross(r_dir, target_dir));
    let v_dir = unit(cross(plane_normal, r_dir));

    State {
        t: T0,
        r: add(earth.r, scale(r_dir, R_LEO)),
        v: add(earth.v, scale(v_dir, (MU_EARTH / R_LEO).sqrt())),
    }
}

/// Розв'язує 3×3 за Крамером. Тут це доречно саме тому, що система маленька
/// й фіксована: жодних півотів, жодної бібліотеки, і видно, що саме рахується.
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };

    let d = det(a);
    assert!(
        d.abs() > 0.0,
        "матриця чутливості вироджена — це не збіг корекції, а її відсутність"
    );

    let mut out = [0.0; 3];
    for (col, value) in out.iter_mut().enumerate() {
        let mut m = a;
        for row in 0..3 {
            m[row][col] = b[row];
        }
        *value = det(m) / d;
    }
    out
}

/// Оракул №1: імпульс, знайдений Ламбертом і виправлений матрицею переходу,
/// приводить апарат у позицію Місяця з ассета.
///
/// Чому це зовнішня задача, а не ще один прогін тієї самої машинерії: ціль —
/// число з ефемериди, початкове наближення — з `lambert_solve`, тобто з коду,
/// який нічого не знає ні про план, ні про світ. Якби імпульс застосовувався
/// не так, як тут вважається, апарат просто не прилетів би.
#[test]
fn a_lambert_burn_corrected_by_the_stm_arrives_where_the_moon_is() {
    let eph = Arc::new(Ephemeris::load(&mission::default_asset()).expect("ассет"));
    let mut prop = Propagator::new(eph.clone(), config()).expect("пропагатор");

    let earth_arrive = eph.body_state(EARTH, T_ARRIVE).expect("Земля");
    let moon_arrive = eph.body_state(MOON, T_ARRIVE).expect("Місяць");
    let target = sub(moon_arrive.r, earth_arrive.r);

    let start = leo_start(&eph, unit(target));

    // Стан у момент запалення — пропагацією з [`T0`], бо саме там гра його
    // й візьме.
    let mut step = 0.0;
    let (at_burn, _) = prop
        .run_stm(&start, None, T_BURN, &mut step)
        .expect("прогін до запалення");

    let earth_burn = eph.body_state(EARTH, T_BURN).expect("Земля");
    let r1 = sub(at_burn.r, earth_burn.r);

    // `prograde` — знак z-компоненти моменту імпульсу, і саме так це тут і
    // рахується, а не вгадується прапорцем. Помилитися в ньому означало б
    // отримати розв'язок іншої задачі, який теж збігається.
    let prograde = cross(r1, target).z > 0.0;

    let (v1, _v2) = lambert_solve(r1, target, T_ARRIVE - T_BURN, MU_EARTH, prograde, 0)
        .expect("двотільний переліт до Місяця існує");

    let mut dv = sub(add(earth_burn.v, v1), at_burn.v);
    assert!(
        norm(dv) < 6.0e3,
        "Ламберт зажадав {:.4e} м/с — стільки коштує поворот площини, а не          переліт. Орбіта відльоту не в площині цілі.",
        norm(dv)
    );

    // Ньютон по Δv: нев'язка — промах по позиції в момент прильоту, похідна —
    // блок ∂r_кінц/∂v_поч матриці переходу.
    //
    // **Крок половинний, і це виміряно, а не обережність.** З повним кроком
    // послідовність промахів стрибає (1.2e7 → 4.4e6 → 2.2e6 → 6.2e6): біля
    // Місяця задача помітно нелінійна, і Ньютон перелітає. З половинним вона
    // монотонна й падає на три порядки. Це та сама нелінійність, через яку
    // реальні місії роблять корекції, а не один точний імпульс.
    const DAMPING: f64 = 0.5;
    let mut miss = Vec::new();
    for _ in 0..8 {
        let burned = State {
            t: T_BURN,
            r: at_burn.r,
            v: add(at_burn.v, dv),
        };

        let mut step = 0.0;
        let (arrived, phi) = prop
            .run_stm(&burned, None, T_ARRIVE, &mut step)
            .expect("прогін до прильоту");

        let residual = sub(arrived.r, moon_arrive.r);
        miss.push(norm(residual));

        let mut jacobian = [[0.0; 3]; 3];
        for (i, row) in jacobian.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = phi.get(i, 3 + j);
            }
        }

        let delta = solve3(jacobian, [-residual.x, -residual.y, -residual.z]);
        dv = add(
            dv,
            scale(
                Vec3d {
                    x: delta[0],
                    y: delta[1],
                    z: delta[2],
                },
                DAMPING,
            ),
        );
    }

    // Двотільне наближення промахується настільки, що виправляти є що; після
    // корекції промах падає на порядки. Обидва твердження потрібні: без
    // першого тест проходив би і на задачі, у якій нічого не робиться.
    // Останнє виправлення ще не перевірене: цикл рахує нев'язку, потім
    // править Δv. Тому фінальний прогін окремо — він дає і промах, і
    // передбачення, з яким далі звіряється гра.
    let mut step = 0.0;
    let (predicted, _) = prop
        .run_stm(
            &State {
                t: T_BURN,
                r: at_burn.r,
                v: add(at_burn.v, dv),
            },
            None,
            T_ARRIVE,
            &mut step,
        )
        .expect("фінальний прогін");
    let final_miss = norm(sub(predicted.r, moon_arrive.r));
    assert!(
        miss[0] > 1.0e6,
        "двотільний Ламберт промахнувся лише на {:.3e} м — тоді корекція \
         нічого не доводить",
        miss[0]
    );
    assert!(
        final_miss < 5.0e4,
        "після семи корекцій промах {final_miss:.3e} м. Послідовність: {miss:?}"
    );

    // --- і те саме через гру ---
    //
    // Той самий Δv, поданий планом. Якщо світ застосовує імпульс інакше, ніж
    // це щойно зробив прогін, апарат прилетить не туди — і байдуже, що обидва
    // рахують той самий інтегратор.
    let mut world = World::with_ephemeris(eph.clone(), config(), T0, mission::DEFAULT_WARP)
        .expect("світ будується");
    let id = world.add_vessel("lambert", start, T_ARRIVE, None);
    assert_eq!(id, VesselId(0));

    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: T_BURN,
        dv: [dv.x, dv.y, dv.z],
        frame: Frame::Inertial,
    });
    world.commit_plan(id, plan).expect("маневр у майбутньому");
    world.run_to_end(1.0, 64);

    let flown = world.vessels()[0].trajectory.state_at(T_ARRIVE);

    // Звіряється з **передбаченням прогону**, а не з Місяцем, і це не
    // послаблення. Питання цього тесту — чи застосовує світ імпульс так само,
    // як його застосував прогін; наскільки точно обидва влучили в Місяць, уже
    // сказано вище. Порівняння з Місяцем змішало б дві різні похибки.
    let drift = norm(sub(flown.r, predicted.r));
    let game_miss = norm(sub(flown.r, moon_arrive.r));

    // Поріг з виміру, не з обережності. Власний дрейф — 25 м: світ ріже
    // прогін на ланки по заповненому буферу, а прогін вище йшов одним
    // викликом, тож послідовності кроків різні, а біля Місяця різниця
    // підсилюється. Помилка в 10⁻⁶ від Δv дає вже 1194 м, у 10⁻⁵ — 1.1·10⁴.
    // П'ятсот метрів лишає двадцятикратний запас над власним дрейфом і все
    // одно ловить найменшу з трьох виміряних мутацій.
    assert!(
        drift < 5.0e2,
        "гра прилетіла за {drift:.3e} м від того, що передбачив прогін із тим \
         самим Δv (промах по Місяцю {game_miss:.3e} м проти {final_miss:.3e} м). \
         Різниця тут — це сегментний цикл або момент застосування імпульсу, \
         а не фізика."
    );
}

/// Оракул №2: базис VNB означає те, що написано.
///
/// Три твердження підручникової двотільної механіки, жодне з яких не
/// виводиться з `dv_inertial`. Головне з них — що прямий і нормальний
/// імпульси **по-різному** міняють швидкість: лінійно проти квадратично.
/// Переставити їх місцями й пройти цей тест неможливо.
#[test]
fn the_vnb_basis_means_what_it_says() {
    let eph = Ephemeris::load(&mission::default_asset()).expect("ассет");
    let moon = eph.body_state(MOON, T_ARRIVE).expect("Місяць");
    let earth_arrive = eph.body_state(EARTH, T_ARRIVE).expect("Земля");
    let vessel = leo_start(&eph, unit(sub(moon.r, earth_arrive.r)));
    let earth = eph.body_state(EARTH, T0).expect("Земля");

    let rel_r = sub(vessel.r, earth.r);
    let rel_v = sub(vessel.v, earth.v);
    let h = cross(rel_r, rel_v);
    let speed = norm(rel_v);

    let burn = 100.0;
    let inertial = |dv: [f64; 3]| {
        Manoeuvre {
            t: T0,
            dv,
            frame: Frame::Vnb { body: EARTH },
        }
        .dv_inertial(&vessel, Some(&earth))
    };

    // --- прямий: паралельний швидкості ---
    let prograde = inertial([burn, 0.0, 0.0]);
    let after = Vec3d {
        x: rel_v.x + prograde[0],
        y: rel_v.y + prograde[1],
        z: rel_v.z + prograde[2],
    };

    assert!(
        (norm(after) - (speed + burn)).abs() < 1e-6,
        "прямий імпульс мав дати |v|+Δv = {:.9e}, дав {:.9e}. Це не швидкість \
         уздовж вектора швидкості.",
        speed + burn,
        norm(after)
    );

    let h_after = cross(rel_r, after);
    let tilt = norm(cross(h, h_after)) / (norm(h) * norm(h_after));
    assert!(
        tilt < 1e-12,
        "прямий імпульс повернув площину орбіти на {tilt:.3e} — а він не має \
         виходити з неї взагалі"
    );

    // --- нормальний: перпендикулярний, отже за Піфагором ---
    let normal = inertial([0.0, burn, 0.0]);
    let after = Vec3d {
        x: rel_v.x + normal[0],
        y: rel_v.y + normal[1],
        z: rel_v.z + normal[2],
    };

    let pythagoras = (speed * speed + burn * burn).sqrt();
    assert!(
        (norm(after) - pythagoras).abs() < 1e-6,
        "нормальний імпульс мав дати sqrt(|v|²+Δv²) = {pythagoras:.9e}, дав \
         {:.9e}. Якщо вийшло |v|+Δv — нормаль і прямий напрямок переставлені.",
        norm(after)
    );

    let h_after = cross(rel_r, after);
    let tilt = norm(cross(h, h_after)) / (norm(h) * norm(h_after));
    assert!(
        tilt > 1e-3,
        "нормальний імпульс не повернув площину орбіти (нахил {tilt:.3e}) — \
         тоді він не нормальний"
    );

    // --- назовні: у площині орбіти й ВІД тіла ---
    let outward = inertial([0.0, 0.0, burn]);
    let outward = Vec3d {
        x: outward[0],
        y: outward[1],
        z: outward[2],
    };

    assert!(
        dot(outward, rel_r) > 0.0,
        "«назовні» вказує до тіла, а не від нього: r·Δv = {:.3e}. Це знак у \
         cross(prograde, normal), тобто орієнтація трійки.",
        dot(outward, rel_r)
    );
    assert!(
        dot(outward, h).abs() / (burn * norm(h)) < 1e-12,
        "«назовні» вийшло з площини орбіти"
    );
    assert!(
        dot(outward, rel_v).abs() / (burn * speed) < 1e-12,
        "«назовні» має складову вздовж швидкості"
    );
}
