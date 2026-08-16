//! Колір — властивість тіла, а не кадру (етап T, крок T1).
//!
//! Оракул навмисно вимагає **двох** тіл. З одним пройшла б і та реалізація,
//! якої крок позбувається: колір їде в uniform із динамічним зсувом на прохід,
//! і «останній викликач виграв» — рівно та помилка, через яку колір ламаних
//! свого часу став атрибутом вершини (J1). Одне тіло її не показує ніяк.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::Shot;
use engine::{frame, shot, sphere};

const SIZE: u32 = 256;

/// Дві планети обабіч осі погляду, кожна зі своїм кольором.
///
/// Радіус земний, рознесені на чотири радіуси, камера — за двадцять. Числа
/// підібрані так, щоб обидва диски цілком влазили в кадр і не торкались:
/// диски, що перекрилися б, зробили б «де чий піксель» питанням глибини, а не
/// кольору.
fn two_bodies(left: [f32; 4], right: [f32; 4]) -> Scene {
    let radius = sphere::EARTH_RADIUS_M;
    let body = |centre: [f64; 3], colour: [f32; 4]| Body {
        centre,
        radius_m: radius,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour,
        air: None,
    };

    let camera = Camera::look_at([20.0 * radius, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = Scene::new(camera);
    scene.bodies.push(body([0.0, 2.0 * radius, 0.0], left));
    scene.bodies.push(body([0.0, -2.0 * radius, 0.0], right));
    scene
}

/// Колонка, у якій камера бачить центр тіла.
///
/// Питається в самої камери, а не виводиться з осей: `+y` світу в цьому кадрі
/// лягає **праворуч** по екрану, і перший підхід до цього тесту припустив
/// протилежне. Здогад про напрямок осі — це рівно те, що оракул має брати з
/// коду, а не з голови.
fn screen_x(scene: &Scene, index: usize) -> f64 {
    let centre = scene.bodies[index].centre;
    let screen = scene
        .camera
        .to_screen(frame::FOV_Y, SIZE, SIZE, centre)
        .expect("тіло позаду камери — сцена не та");
    f64::from(screen[0])
}

/// Середня колонка пікселів, у яких перший канал переважає третій (або
/// навпаки), і скільки їх.
fn centroid(shot: &Shot, red: bool) -> (f64, usize) {
    let mut count = 0usize;
    let mut sum = 0.0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
                continue;
            }
            let dominant = if red { p[0] > p[2] } else { p[2] > p[0] };
            if dominant {
                count += 1;
                sum += f64::from(x);
            }
        }
    }
    (sum / count.max(1) as f64, count)
}

/// Два тіла різних кольорів дають у кадрі два кольори, і кожен на своєму боці.
///
/// Двох тверджень мало по одному: «обидва кольори є» пройшло б і тоді, коли
/// вони помінялися місцями, а «ліве ліворуч» — коли обидва тіла сірі й
/// класифікація ловить шум. Разом вони називають і колір, і власника.
#[test]
fn two_bodies_keep_their_own_colours() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let red = [0.9, 0.1, 0.1, 1.0];
    let blue = [0.1, 0.1, 0.9, 1.0];
    let scene = two_bodies(red, blue);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр із двома тілами");

    let (red_x, red_n) = centroid(&shot, true);
    let (blue_x, blue_n) = centroid(&shot, false);

    // Диск земного радіуса з двадцяти радіусів — близько 850 пікселів у кадрі
    // 256×256. Поріг на порядок нижчий: він відрізняє «тіло є» від «кілька
    // пікселів шуму на термінаторі», а не міряє площу.
    assert!(red_n > 100, "червоних пікселів лише {red_n}");
    assert!(blue_n > 100, "синіх пікселів лише {blue_n}");
    // Де кожне тіло — знає камера. Червоне те, що першим у списку.
    let (want_red, want_blue) = (screen_x(&scene, 0), screen_x(&scene, 1));
    assert!(
        (red_x - want_red).abs() < 20.0,
        "червоний центр у колонці {red_x}, а тіло — в {want_red}"
    );
    assert!(
        (blue_x - want_blue).abs() < 20.0,
        "синій центр у колонці {blue_x}, а тіло — в {want_blue}"
    );

    // І навпаки: помінявши кольори місцями, кадр мусить помінятися теж.
    // Без цього перевірка вище пройшла б і на кадрі, де колір не читається з
    // тіла взагалі, а береться з порядку малювання.
    let swapped = two_bodies(blue, red);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &swapped).expect("кадр із двома тілами");
    let (red_x, _) = centroid(&shot, true);
    let (blue_x, _) = centroid(&shot, false);
    assert!(
        (red_x - want_blue).abs() < 20.0 && (blue_x - want_red).abs() < 20.0,
        "колір не пішов за тілом: червоний у {red_x}, синій у {blue_x}"
    );
}
