//! Корпус освітлений звідти ж, звідки планета під ним (етап V, крок V5;
//! борг D16).
//!
//! Оракул — **бік**, а не яскравість: у кадрі рахується центр ваги світла
//! окремо для корабля й окремо для поверхні, і обидва мусять зсунутися в один
//! бік від своїх геометричних центрів. Число тут не «скільки люмінансу», а
//! куди він поїхав; яскравість залежить від матеріалу, а бік — від світила.
//!
//! Це й ловить те, чим борг D16 був насправді небезпечний: доки напрямок був
//! сталою рушія, корпус і небо могли світитися з різних боків, і жодна
//! перевірка про це не питала.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, Ship, TileSet};
use engine::shot::Shot;
use engine::{frame, ship, shot, sphere};

const SIZE: u32 = 256;

/// Висота, з якої корабель видно на тлі поверхні, метри.
const ALTITUDE_M: f64 = 400_000.0;

/// Скільки метрів від камери до корабля.
const RANGE_M: f64 = 15.0;

/// Сцена: корабель перед камерою, планета під ним, світило — звідки скажуть.
///
/// Повітря немає навмисно. Небо накрило б і корпус, і поверхню власним
/// розсіянням, і тест міряв би аеральну перспективу замість дифузного члена.
fn scene_lit_from(sun: [f64; 3]) -> Scene {
    scene_of(sun, true, true)
}

/// Та сама сцена, але з вибором, що в ній є.
///
/// Потрібна для **масок**: силует корабля береться з кадру, де планети немає
/// взагалі, і навпаки. Класифікувати піксель за кольором більше не можна —
/// див. [`lit_offset`].
fn scene_of(sun: [f64; 3], with_body: bool, with_ship: bool) -> Scene {
    let radius = sphere::EARTH_RADIUS_M + ALTITUDE_M;
    // Камера дивиться вниз під кутом: у кадрі тоді і корабель, і поверхня.
    let centre = [radius, 0.0, 0.0];
    let eye = [radius + 0.6 * RANGE_M, -0.8 * RANGE_M, 0.0];
    let camera = Camera::look_at(eye, centre, [1.0, 0.0, 0.0]);

    let mut scene = Scene::new(camera);
    scene.sun = sun;
    if with_body {
        scene.bodies.push(Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: sphere::EARTH_RADIUS_M,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
            colour: frame::COLOUR,
            air: None,
        });
    }
    if !with_ship {
        return scene;
    }
    scene.ships.push(Ship {
        centre,
        orientation: [1.0, 0.0, 0.0, 0.0],
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        // Матеріал — той самий, що в решті фікстур рушія. Питання тесту про
        // бік, а не про яскравість, тож дзеркальний відблиск його не псує:
        // при `n·l ≤ 0` BRDF дає нуль незалежно від матеріалу, і темний бік
        // лишається темним.
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    });
    scene
}

fn luminance(p: [u8; 4]) -> f64 {
    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
}

/// Силует: які пікселі кадру не є небом.
///
/// ⚠ **Маска геометрична, а не колірна, і це виправлення, а не ускладнення.**
/// Перша редакція розводила корабель і планету за відношенням каналів — у
/// корпусу `r ≈ b`, у планети `r = 0.22·b`. Це працювало рівно доти, доки в
/// освітленні був ambient 0.05 і жоден піксель не був чорним. З нульовим
/// ambient (T5c, PROJECT.md §7) неосвітлена половина корпусу стала рівно
/// `[0, 0, 0]`, а `0 > 0` хибне — тобто **тінь корабля рахувалася планетою**,
/// і центр ваги поверхні їхав на вісімдесят пікселів. Маска з окремого кадру
/// такої залежності не має взагалі.
fn silhouette(shot: &Shot) -> Vec<bool> {
    let mut out = vec![false; (shot.width * shot.height) as usize];
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            out[(y * shot.width + x) as usize] = [p[0], p[1], p[2]] != frame::CLEAR_BYTES;
        }
    }
    out
}

/// Куди зсунувся центр ваги світла від геометричного центра позначених
/// пікселів, у пікселях екрана.
///
/// Саме різниця двох центрів, а не сам центр ваги: силует корабля
/// несиметричний, і його центр ваги зсунутий уже без будь-якого освітлення.
fn lit_offset(shot: &Shot, mask: &[bool]) -> [f64; 2] {
    let mut area = (0.0, 0.0, 0.0);
    let mut light = (0.0, 0.0, 0.0);
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if !mask[(y * shot.width + x) as usize] {
                continue;
            }
            let (fx, fy) = (f64::from(x), f64::from(y));
            area = (area.0 + 1.0, area.1 + fx, area.2 + fy);
            let l = luminance(p);
            light = (light.0 + l, light.1 + l * fx, light.2 + l * fy);
        }
    }
    assert!(area.0 > 100.0, "позначених пікселів лише {}", area.0);
    assert!(light.0 > 0.0, "жоден позначений піксель не освітлений");
    [
        light.1 / light.0 - area.1 / area.0,
        light.2 / light.0 - area.2 / area.0,
    ]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// Корпус і поверхня світяться з одного боку, і обидва слухаються світила.
///
/// Два світила — протилежні одне одному впоперек кадру. Для кожного бік
/// корпусу мусить збігтися з боком поверхні (додатний скалярний добуток), а
/// між світилами обидва боки мусять **перевернутися**. Однієї з цих умов
/// мало: збіг без перевертання виконався б і тоді, коли світло не доїхало
/// нікуди, а перевертання без збігу — коли корпус і планета читають різні
/// напрямки, тобто рівно за боргу D16.
#[test]
fn the_hull_and_the_surface_are_lit_from_the_same_side() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let mut hull = Vec::new();
    let mut surface = Vec::new();
    for sign in [1.0, -1.0] {
        // Упоперек погляду й трохи назустріч: чисто бічне світло лишило б
        // половину кадру зовсім чорною, і центр ваги рахувати не було б у чому.
        //
        // ⚠ Перевертається саме `z`, і це складова, яка в цьому кадрі лягає
        // **горизонтально** (камера дивиться вздовж світової `x`, і та йде в
        // екранну вертикаль). Спроба зробити «бічну складову головною», взявши
        // `[±0.8, 0, 0.6]`, перевертає вертикальну складову, а горизонтальний
        // зсув лишає сталим — і перевірка на перевертання падає, хоч фізика
        // правильна.
        let sun = [0.4, 0.0, sign * 0.92];
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_lit_from(sun)).expect("кадр");

        // Маски — з кадрів, де є щось одне. Корабель ближчий за планету, тож
        // його силует накриває її: з маски поверхні він віднімається.
        let only_ship = shot::take_scene(&gpu, SIZE, SIZE, &scene_of(sun, false, true))
            .expect("кадр самого корабля");
        let only_body = shot::take_scene(&gpu, SIZE, SIZE, &scene_of(sun, true, false))
            .expect("кадр самої планети");
        let hull_mask = silhouette(&only_ship);
        let body_mask: Vec<bool> = silhouette(&only_body)
            .iter()
            .zip(&hull_mask)
            .map(|(body, ship)| *body && !*ship)
            .collect();

        hull.push(lit_offset(&shot, &hull_mask));
        surface.push(lit_offset(&shot, &body_mask));
    }

    for k in 0..2 {
        assert!(
            dot(hull[k], surface[k]) > 0.0,
            "світило {k}: корпус зсунувся в {:?}, поверхня в {:?}",
            hull[k],
            surface[k]
        );
    }
    assert!(
        dot(hull[0], hull[1]) < 0.0,
        "корпус не помітив, що світило перейшло на інший бік: {hull:?}"
    );
    assert!(
        dot(surface[0], surface[1]) < 0.0,
        "поверхня не помітила, що світило перейшло на інший бік: {surface:?}"
    );
}
