//! Два діапазони глибини вперше в справжній сцені (етап V, крок V3).
//!
//! До цього кроку `plan` рахував один прохід завжди: сцена зондів рушія має
//! розмах 22.7, тобто менший за один буфер глибини (F3: сім порядків). Кадр
//! із корпусом за метри й планетою за мільйони метрів — перший, у якому їх
//! два, і перша перевірка того, заради чого Q2 лишив діапазони в конструкції.
//!
//! Тут перевіряється те, що можна побачити лише на GPU: чи цілий корпус і чи
//! немає шва там, де діапазони сходяться. Арифметика самої межі — юніт-тести
//! `engine::frame` (`the_range_boundary_never_falls_inside_the_hull`).

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, Ship, TileSet};
use engine::shot::Shot;
use engine::{frame, ship, shot, sphere};

const SIZE: u32 = 256;
const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const ASPECT: f64 = 1.0;

/// Висота камери над ґрунтом у сцені [`ship_over_the_ground`], метри.
const EYE_ALTITUDE_M: f64 = 1000.0;

/// Скільки метрів від камери до корабля в тій самій сцені, упоперек і вниз.
const GROUND_RANGE_M: f64 = 12.0;
const GROUND_DROP_M: f64 = 4.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

fn earth() -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: frame::COLOUR,
        // Без повітря навмисно: фон лишається кольором очищення, тож будь-яка
        // дірка в кадрі видна як дірка. Небо накрило б її собою.
        air: None,
    }
}

fn hull(centre: [f64; 3], orientation: [f64; 4]) -> Ship {
    Ship {
        centre,
        orientation,
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: engine::ship::HULL_ROUGHNESS,
        metallic: engine::ship::HULL_METALLIC,
    }
}

fn drawn(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// Прямокутник, у який вписані всі намальовані пікселі.
fn lit_bounds(shot: &Shot) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..shot.height {
        for x in 0..shot.width {
            if !drawn(shot, x, y) {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bounds
}

/// Силует того самого меша, порахований на CPU — оракул V2, без змін, окрім
/// одного: корабель тут не в початку координат, тож меш треба перенести туди,
/// де він стоїть. Поворот тотожний, і це вибір сцени, а не спрощення оракула.
fn projected_bounds(camera: &Camera, height_m: f64, centre: [f64; 3]) -> (f64, f64, f64, f64) {
    let mesh = ship::generate(height_m);
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &mesh.positions {
        let world = [p[0] + centre[0], p[1] + centre[1], p[2] + centre[2]];
        let screen = camera
            .to_screen(FOV_Y, SIZE, SIZE, world)
            .expect("вершина позаду камери — сцена не та");
        bounds.0 = bounds.0.min(f64::from(screen[0]));
        bounds.1 = bounds.1.min(f64::from(screen[1]));
        bounds.2 = bounds.2.max(f64::from(screen[0]));
        bounds.3 = bounds.3.max(f64::from(screen[1]));
    }
    bounds
}

/// Корабель на орбіті, планета **за спиною камери**.
///
/// Диск Землі з 400 км займає 70.2° від надира, тобто з кадру, спрямованого в
/// зеніт, він випадає цілком при будь-якому куті огляду до 109°. Проходів при
/// цьому все одно два: `far_for` міряє розмах сцени, а не того, що видно —
/// планета лишається в сцені й тягне далеку межу на 1.3·10⁷ м.
///
/// Тому в кадрі є рівно корабель, і його силует можна порівняти з проєкцією
/// тим самим оракулом, що в V2 — але тепер із двома діапазонами, а не одним.
fn ship_against_space() -> Scene {
    let radius = sphere::EARTH_RADIUS_M + 400_000.0;
    let eye = [radius, 0.0, 0.0];
    let centre = [radius + 15.0, 0.0, 0.0];
    let camera = Camera::look_at(eye, centre, [0.0, 0.0, 1.0]);

    let mut scene = Scene::new(camera);
    scene.bodies.push(earth());
    scene.ships.push(hull(centre, [1.0, 0.0, 0.0, 0.0]));
    scene
}

/// Місцевий базис сцени з ґрунтом: `up` — від центра планети, `east` і
/// `north` — упоперек.
///
/// Напрямок косий навмисно: фікстура, що стоїть рівно над центром грані куба,
/// уже ховала дві помилки поспіль (D13, D14).
fn ground_basis() -> ([f64; 3], [f64; 3], [f64; 3]) {
    let unit = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / n, v[1] / n, v[2] / n]
    };
    let up = unit([0.37, -0.51, 0.77]);
    let east = unit([-up[1], up[0], 0.0]);
    let north = [
        up[1] * east[2] - up[2] * east[1],
        up[2] * east[0] - up[0] * east[2],
        up[0] * east[1] - up[1] * east[0],
    ];
    (up, east, north)
}

/// Корабель поруч із камерою, обидва за кілометр над ґрунтом.
///
/// Це вигляд від третьої особи в польоті, і сцена потрібна саме тому, що в ній
/// **межа діапазонів ріже видиму поверхню посеред кадру**: near = 0.96 м
/// (десята частина відстані до корпусу), far = 1.27·10⁷ м, тобто межа стоїть
/// за 3.51 км, а горизонт із кілометра — за 113 км.
///
/// ## Чому кілометр, а не десять метрів — і чому це не смак
///
/// Висота камери **скорочується**, і в цьому вся річ. `near` — десята частина
/// висоти, `far` — приблизно діаметр планети, тож для камери **без корабля**
/// межа завжди виходить `√(2R·h/10)`, тобто рівно `горизонт/3.16`. Ґрунт на
/// такій відстані стоїть під кутом 1.58 нахилу горизонту — а сам нахил на
/// десяти метрах це 1.77 мрад проти пікселя в 4.09 мрад. Тобто вся ділянка
/// другого діапазону лежить **у чверті пікселя** під горизонтом, і перевірити
/// там не можна нічого: виміряно зламом, розсунуті вчетверо площини лишали
/// кадр без жодної дірки.
///
/// Корабель за дванадцять метрів розриває це співвідношення: `near` тепер
/// його, а не висоти, тож межа лишається на трьох із половиною кілометрах,
/// поки горизонт іде на сто тринадцять. Ґрунт на межі стоїть під 16°,
/// горизонт — під 1°, і між ними шістдесят рядків кадру.
///
/// Корабель опущений на чотири метри нижче камери навмисно: так увесь корпус
/// лежить **під** лінією горизонту. Корабель, що стирчить у небо, дає в своїх
/// стовпцях законні пікселі фону під носом, і оракул шва довелося б робити
/// складнішим за те, що він перевіряє.
fn ship_over_the_ground() -> Scene {
    let radius = sphere::EARTH_RADIUS_M;
    let (up, east, _) = ground_basis();

    let at = |altitude: f64, east_m: f64| {
        let r = radius + altitude;
        [
            up[0] * r + east[0] * east_m,
            up[1] * r + east[1] * east_m,
            up[2] * r + east[2] * east_m,
        ]
    };

    let eye = at(EYE_ALTITUDE_M, 0.0);
    let centre = at(EYE_ALTITUDE_M - GROUND_DROP_M, GROUND_RANGE_M);
    let camera = Camera::look_at(eye, centre, up);

    let mut scene = Scene::new(camera);
    scene.bodies.push(earth());
    scene.ships.push(hull(centre, [1.0, 0.0, 0.0, 0.0]));
    scene
}

/// Обидві сцени справді дають два діапазони — інакше решта файлу перевіряє
/// однопрохідний кадр і мовчить про це.
///
/// Друге твердження — про сцену з ґрунтом, і воно й робить перевірку шва
/// непорожньою: **межа мусить лягти між камерою й горизонтом**. Горизонт
/// сфери — точна формула, `√(2Rh)`, без наближень, тож і це число сцени, а не
/// смак. Ляже межа далі — шва не буде з тієї простої причини, що обидва
/// діапазони не сходяться ніде у видимому кадрі.
#[test]
fn both_scenes_ask_for_two_depth_ranges() {
    let orbit = frame::Frame::depth_ranges(&ship_against_space(), ASPECT);
    assert_eq!(orbit.len(), 2, "корабель на орбіті: {orbit:?}");

    let ground = frame::Frame::depth_ranges(&ship_over_the_ground(), ASPECT);
    assert_eq!(ground.len(), 2, "корабель на ґрунті: {ground:?}");

    let horizon = (2.0 * sphere::EARTH_RADIUS_M * EYE_ALTITUDE_M).sqrt();
    assert!(
        ground[1] < horizon,
        "межа {} м лежить за горизонтом ({horizon} м) — ґрунту з другого \
         діапазону в кадрі немає, і шву нема де з'явитися",
        ground[1]
    );
}

/// Другий діапазон не забирає корпус: силует той самий, що й без нього.
///
/// Оракул — проєкція меша через `Camera::to_screen`, тобто рівно той, яким
/// V2 міряв корабель в однопрохідному кадрі. Допуск асиметричний із тієї ж
/// причини: растеризатор здатен загубити вістря, але не намалювати поза
/// геометрією.
#[test]
fn two_ranges_do_not_clip_the_hull() {
    let Some(gpu) = gpu() else {
        return;
    };

    let scene = ship_against_space();
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр з кораблем");
    let (x0, y0, x1, y1) = lit_bounds(&shot).expect("у кадрі порожньо — корабля немає");
    let expected = projected_bounds(&scene.camera, ship::DEFAULT_HEIGHT_M, scene.ships[0].centre);

    let inside = |what: &str, drawn: f64, want: f64, sign: f64| {
        let over = sign * (drawn - want);
        assert!(
            over <= 1.0,
            "{what}: кадр вийшов за проєкцію на {over} px ({drawn} проти {want})"
        );
        assert!(
            over >= -2.5,
            "{what}: кадр не дотягнув до проєкції {} px ({drawn} проти {want})",
            -over
        );
    };
    inside("ліворуч", f64::from(x0), expected.0, -1.0);
    inside("вгорі", f64::from(y0), expected.1, -1.0);
    inside("праворуч", f64::from(x1), expected.2, 1.0);
    inside("внизу", f64::from(y1), expected.3, 1.0);
}

/// Рядок горизонту в кожному стовпці кадру — **з геометрії, а не з кадру**.
///
/// Дотичні точки сфери з ока на радіусі `r` лежать на колі, і воно виражається
/// точно: `E·T = R²` для `|T| = R`, звідки `T(φ) = (R²/r)·up + R·√(1−R²/r²)·w(φ)`.
/// Спроєктований `Camera::to_screen`, цей набір і є лінією горизонту.
///
/// Питати про неї кадр не можна, і це головний урок цієї перевірки: дірка
/// **на самому горизонті** просто зсуває «перший намальований піксель» униз, і
/// оракул, який бере горизонт із кадру, не бачить її взагалі. Виміряно
/// зламом: розсунуті на порядок площини забирають шість рядків ґрунту, а
/// перевірка «нижче першого намальованого дірок немає» лишається зеленою.
fn horizon_rows(camera: &Camera, altitude_m: f64) -> Vec<Option<f64>> {
    let radius = sphere::EARTH_RADIUS_M;
    let r = radius + altitude_m;
    let (up, east, north) = ground_basis();

    let along_up = radius * radius / r;
    let across = radius * (1.0 - (radius / r) * (radius / r)).sqrt();

    let mut rows: Vec<Option<f64>> = vec![None; SIZE as usize];
    // Кроків більше, ніж стовпців, і з великим запасом: горизонт перетинає
    // кадр навскіс, тож рідка вибірка лишила б стовпці без відповіді.
    let steps = 20_000;
    for k in 0..steps {
        let phi = 2.0 * std::f64::consts::PI * f64::from(k) / f64::from(steps);
        let w = [
            phi.cos() * east[0] + phi.sin() * north[0],
            phi.cos() * east[1] + phi.sin() * north[1],
            phi.cos() * east[2] + phi.sin() * north[2],
        ];
        let point = [
            along_up * up[0] + across * w[0],
            along_up * up[1] + across * w[1],
            along_up * up[2] + across * w[2],
        ];
        let Some(screen) = camera.to_screen(FOV_Y, SIZE, SIZE, point) else {
            continue;
        };
        let x = f64::from(screen[0]);
        if !(0.0..f64::from(SIZE)).contains(&x) {
            continue;
        }
        let slot = &mut rows[x as usize];
        let y = f64::from(screen[1]);
        *slot = Some(slot.map_or(y, |old: f64| old.min(y)));
    }
    rows
}

/// Там, де діапазони сходяться, поверхня не рветься — і доходить до самого
/// горизонту.
///
/// Два твердження, і друге важливіше за перше:
///
/// - **ґрунт починається на горизонті**, а не там, де вийшло. Саме сюди
///   впав би шов на межі діапазонів: у цій сцені межа стоїть за 3.57 км, а
///   горизонт — за 11.3 км, тобто ділянка другого діапазону тонка й лежить
///   упритул до горизонту;
/// - **нижче горизонту дірок немає жодної.**
///
/// Допуск — піксель: горизонт лягає між рядками, і растеризатор фарбує той,
/// центр якого накрито.
#[test]
fn the_surface_has_no_seam_where_the_ranges_meet() {
    let Some(gpu) = gpu() else {
        return;
    };

    let scene = ship_over_the_ground();
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр із ґрунтом");
    let horizon = horizon_rows(&scene.camera, EYE_ALTITUDE_M);

    let mut checked = 0;
    for x in 0..shot.width {
        let Some(row) = horizon[x as usize] else {
            continue;
        };
        // Стовпці, у яких горизонт не влазить у кадр, нічого не стверджують.
        if !(1.0..f64::from(shot.height) - 1.0).contains(&row) {
            continue;
        }
        checked += 1;

        let first = (row.ceil() as u32 + 1).min(shot.height - 1);
        for y in first..shot.height {
            assert!(
                drawn(&shot, x, y),
                "стовпець {x}: під горизонтом (рядок {row}) дірка в рядку {y}"
            );
        }
        // І навпаки: над горизонтом ґрунту немає, інакше «дірок немає»
        // виконувалось би тим, що намальовано геть усе.
        let above = row.floor() as u32;
        assert!(
            above < 2 || !drawn(&shot, x, above - 2),
            "стовпець {x}: над горизонтом (рядок {row}) щось намальовано"
        );
    }

    assert!(
        checked > SIZE / 2,
        "горизонт перевірено лише в {checked} стовпцях із {SIZE} — сцена не та"
    );
}
