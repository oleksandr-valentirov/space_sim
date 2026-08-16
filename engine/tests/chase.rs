//! Камера третьої особи справді показує, як корабель повертається (етап V,
//! крок V4).
//!
//! Оракул — **частка змінених пікселів**, а не «виглядає інакше». Камера при
//! цьому стоїть нерухомо: рухається тільки корабель, тож усе, що змінилося в
//! кадрі, змінив саме поворот.
//!
//! Це й перевіряє головне рішення `engine::chase`: камера бере від корабля
//! позицію, а не орієнтацію. Прив'язана до осей корабля, вона дала б нуль тут
//! у всіх трьох рядках одразу.

use engine::chase::Chase;
use engine::gpu::Gpu;
use engine::scene::{Scene, Ship};
use engine::shot::Shot;
use engine::{frame, ship, shot};

const SIZE: u32 = 256;

/// Корабель стоїть далеко від початку координат — там, де `f32` уже нічого не
/// тримає, а camera-relative тримає (F4).
const CENTRE: [f64; 3] = [4.1e6, -2.7e6, 3.3e6];

/// Орієнтир «вгору» — косий, щоб жодна вісь корабля не збіглася з віссю
/// екрана: симетрична фікстура вже ховала дві помилки поспіль (D13, D14).
fn up() -> [f64; 3] {
    let v = [0.37, -0.51, 0.77_f64];
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

fn scene_with(orientation: [f64; 4]) -> Scene {
    let ship = Ship {
        centre: CENTRE,
        orientation,
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
    };
    // Порожнє небо навмисно: жодного тіла, жодної ламаної. Те, що змінилося в
    // кадрі, могло змінити тільки повернення корабля.
    let mut scene = Scene::new(Chase::default().camera(&ship, up()));
    scene.ships.push(ship);
    scene
}

/// Кватерніон повороту на `angle` навколо осі `axis`.
fn turn(axis: [f64; 3], angle: f64) -> [f64; 4] {
    let half = 0.5 * angle;
    let (s, c) = half.sin_cos();
    [c, s * axis[0], s * axis[1], s * axis[2]]
}

fn drawn(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// Скільки пікселів силуету змінилося, у частках від самого силуету.
///
/// Знаменник — об'єднання двох силуетів, а не весь кадр: корабель займає в
/// кадрі кілька відсотків, і частка від кадру говорила б про поле зору, а не
/// про поворот.
fn silhouette_change(a: &Shot, b: &Shot) -> f64 {
    let mut union = 0usize;
    let mut differing = 0usize;
    for y in 0..a.height {
        for x in 0..a.width {
            let (left, right) = (drawn(a, x, y), drawn(b, x, y));
            if left || right {
                union += 1;
            }
            if left != right {
                differing += 1;
            }
        }
    }
    assert!(union > 0, "у кадрі немає корабля взагалі");
    differing as f64 / union as f64
}

/// Прямокутник, у який вписаний силует.
fn bounds(shot: &Shot) -> (u32, u32, u32, u32) {
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
    bounds.expect("у кадрі немає корабля")
}

/// Поворот навколо кожної з трьох осей видно в кадрі.
///
/// Кут — 40°, а не 90°, і це не смак: **стабілізаторів чотири**, тож чверть
/// оберту навколо носа переводить силует сам у себе, і крен виглядав би
/// нерухомим за будь-якої правильної камери. Сорок градусів не збігаються з
/// жодною симетрією меша.
///
/// Виміряно, у частках силуету: **0.578 навколо x, 0.305 навколо y і 0.107
/// навколо z**. Третє число менше не через камеру, а через форму: корпус —
/// тіло обертання, тож крен видно лише стабілізаторами й ілюмінатором. Рівно
/// це V1 і виміряв на самому меші, коли викинутий ілюмінатор обвалив
/// неузгодженість крену до 8·10⁻¹⁶.
///
/// Поріг — 0.05, удвічі нижчий за найслабше з трьох: він ловить «камера
/// повернулася разом із кораблем» (нуль) і «поворот не доїхав до GPU» (теж
/// нуль), а не міряє точні числа, яким нема на що спертися.
#[test]
fn every_axis_of_rotation_changes_the_silhouette() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let angle = 40.0_f64.to_radians();
    let upright = shot::take_scene(&gpu, SIZE, SIZE, &scene_with([1.0, 0.0, 0.0, 0.0]))
        .expect("кадр з кораблем");

    for (name, axis) in [
        ("x", [1.0, 0.0, 0.0]),
        ("y", [0.0, 1.0, 0.0]),
        ("z", [0.0, 0.0, 1.0]),
    ] {
        let turned = shot::take_scene(&gpu, SIZE, SIZE, &scene_with(turn(axis, angle)))
            .expect("кадр з кораблем");
        let change = silhouette_change(&upright, &turned);
        assert!(
            change > 0.05,
            "поворот навколо {name}: силует змінився лише на {change}"
        );
    }
}

/// Камера тримає корабель у кадрі, як би він не крутився.
///
/// Друга половина того самого твердження: поворот міняє силует, але не тягне
/// його з кадру — інакше «змінилося все» означало б, що корабель просто
/// поїхав за край. Перевіряється центр описаного прямокутника: він мусить
/// лишитися в центрі кадру з точністю до розміру самого корабля.
#[test]
fn the_ship_stays_in_the_middle_however_it_turns() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let angle = 40.0_f64.to_radians();
    for orientation in [
        [1.0, 0.0, 0.0, 0.0],
        turn([1.0, 0.0, 0.0], angle),
        turn([0.0, 1.0, 0.0], angle),
        turn([0.0, 0.0, 1.0], angle),
    ] {
        let shot =
            shot::take_scene(&gpu, SIZE, SIZE, &scene_with(orientation)).expect("кадр з кораблем");
        let (x0, y0, x1, y1) = bounds(&shot);
        let centre = [
            0.5 * f64::from(x0 + x1) - 0.5 * f64::from(SIZE),
            0.5 * f64::from(y0 + y1) - 0.5 * f64::from(SIZE),
        ];
        // Допуск — половина висоти силуету: центр описаного прямокутника не
        // збігається з центром корабля (ніс довший за хвіст), і вимагати
        // більшого означало б перевіряти форму меша, а не камеру.
        let tolerance = 0.5 * f64::from(y1 - y0 + 1);
        assert!(
            centre[0].abs() < tolerance && centre[1].abs() < tolerance,
            "корабель зсунувся на {centre:?} px за допуску {tolerance}"
        );
    }
}
