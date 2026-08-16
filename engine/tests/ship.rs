//! Корабель у кадрі: він там є, він потрібного розміру й він повертається
//! (етап V, крок V2).
//!
//! Оракул — не «щось намальовано», а **число проти числа**, як у F5: у
//! проєкції висота предмета в пікселях виражається точно, без наближень.
//! Ніс і хвіст лежать на однаковій відстані від камери за побудовою сцени,
//! тож `y_view / (−z_view)` для них — це рівно `±(h/2)/d`.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Scene, Ship};
use engine::shot::Shot;
use engine::{frame, ship, shot};

const SIZE: u32 = 256;
const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const DISTANCE: f64 = 15.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// Сцена: порожнє небо й один корабель за [`DISTANCE`] метрів перед камерою.
///
/// Порожнє навмисно — жодного тіла, жодної ламаної. Те, що видно в кадрі,
/// може бути тільки кораблем, і жоден інший малювальник не може випадково
/// дати ті самі пікселі.
fn scene_with(orientation: [f64; 4]) -> Scene {
    let eye = [DISTANCE, 0.0, 0.0];
    // Вгору — світовий `+Z`, тобто вісь корабля лягає вертикально в кадрі.
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let mut scene = Scene::new(camera);
    scene.ships.push(Ship {
        centre: [0.0, 0.0, 0.0],
        orientation,
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
    });
    scene
}

/// Прямокутник, у який вписані всі непорожні пікселі: `(x0, y0, x1, y1)`.
fn lit_bounds(shot: &Shot) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
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

/// Той самий силует, порахований на CPU: кожна вершина меша йде через
/// [`Camera::to_screen`] — ту саму функцію, якою піккінг ловить вузли
/// маневрів (U4b).
///
/// Це не «оцінка кутом». Кутова оцінка тут була б **неправильною**, і це
/// вимір, а не теорія: стабілізатор, повернутий до камери, виступає на
/// 2.28 м уперед, тобто проєктується більшим за ніс, який стоїть далі. Ніс
/// дає 88.7 пікселя, а кадр — 96, і зайві сім із половиною саме звідти.
///
/// Тобто оракул тут той самий, що в `cull` проти `cull.slang`: дві незалежні
/// реалізації одного перетворення мусять дати одне число.
fn projected_bounds(camera: &Camera, height_m: f64) -> (f64, f64, f64, f64) {
    let mesh = ship::generate(height_m);
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &mesh.positions {
        let screen = camera
            .to_screen(FOV_Y, SIZE, SIZE, *p)
            .expect("вершина позаду камери — сцена не та");
        bounds.0 = bounds.0.min(f64::from(screen[0]));
        bounds.1 = bounds.1.min(f64::from(screen[1]));
        bounds.2 = bounds.2.max(f64::from(screen[0]));
        bounds.3 = bounds.3.max(f64::from(screen[1]));
    }
    bounds
}

#[test]
fn the_ship_fills_exactly_the_pixels_the_projection_says() {
    let Some(gpu) = gpu() else {
        return;
    };

    let scene = scene_with([1.0, 0.0, 0.0, 0.0]);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр з кораблем");
    let (x0, y0, x1, y1) = lit_bounds(&shot).expect("у кадрі порожньо — корабля немає");
    let expected = projected_bounds(&scene.camera, ship::DEFAULT_HEIGHT_M);

    // Допуск асиметричний, і це не поблажка, а те, що растеризатор справді
    // робить. Піксель фарбується, коли накрито його **центр**, тож біля
    // вістря — носового конуса й кінчика стабілізатора — останній піксель або
    // два не набираються. Назовні ж за крайню вершину силует вийти не може
    // взагалі: там немає геометрії.
    //
    // Тобто твердження сильніше за «приблизно збігається»: не більше за
    // проєкцію ніде, і не менше ніж на два з половиною пікселя.
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

#[test]
fn turning_the_ship_turns_it_in_the_frame() {
    let Some(gpu) = gpu() else {
        return;
    };

    let upright = scene_with([1.0, 0.0, 0.0, 0.0]);
    // Чверть оберту навколо світового `+X`, тобто навколо осі погляду: вісь
    // корабля лягає горизонтально.
    let half = std::f64::consts::FRAC_PI_4;
    let sideways = scene_with([half.cos(), half.sin(), 0.0, 0.0]);

    let a = shot::take_scene(&gpu, SIZE, SIZE, &upright).expect("кадр");
    let b = shot::take_scene(&gpu, SIZE, SIZE, &sideways).expect("кадр");

    let (ax0, ay0, ax1, ay1) = lit_bounds(&a).expect("корабля немає");
    let (bx0, by0, bx1, by1) = lit_bounds(&b).expect("корабля немає");

    let tall = f64::from(ay1 - ay0 + 1) / f64::from(ax1 - ax0 + 1);
    let wide = f64::from(by1 - by0 + 1) / f64::from(bx1 - bx0 + 1);

    // Стоячий корабель вищий, ніж ширший; покладений — навпаки. Одного
    // порівняння з одиницею мало: оракул мусить упасти й тоді, коли поворот
    // не доїхав до GPU взагалі, тобто коли обидва числа однакові.
    assert!(tall > 1.2, "стоячий корабель має бути високим: {tall}");
    assert!(wide < 0.8, "покладений корабель має бути широким: {wide}");
}

/// Сцена без кораблів — це кадр до кроку V2, і не «майже».
///
/// Найдешевший сторож проти того, що новий пайплайн щось малює завжди:
/// порожній список кораблів не має давати жодного пікселя, а `--shot` зондів
/// рушія — лишатись `30812bf2…`.
#[test]
fn a_scene_without_ships_draws_nothing_new() {
    let Some(gpu) = gpu() else {
        return;
    };

    let eye = [DISTANCE, 0.0, 0.0];
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let scene = Scene::new(camera);

    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр");
    assert!(
        lit_bounds(&shot).is_none(),
        "порожня сцена намалювала щось, чого в ній немає"
    );
}
