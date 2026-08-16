//! Корабель-заглушка: процедурний меш, без ассета.
//!
//! Етап «корабель у сцені» стоїть на трьох оракулах — `near`, що приходить зі
//! `Scene`, кількість проходів глибини й камера, — і **форма корпусу до
//! жодного з них не входить**. Тому геометрія тут породжується кодом.
//!
//! З T5d3 у кадру є й скукований корпус з Blender (`Frame::load_ship`), але
//! цей меш нікуди не подівся й **лишається робочою фікстурою**: у нього є
//! аналітичний об'єм ([`volume_m3`]), якого в імпортованої моделі немає
//! взагалі. Кадр малює його й тоді, коли асета немає на диску — `/assets/`
//! не в git.
//!
//! ⚠ **Форма навмисно несиметрична, і це не оздоблення.** Куля була б
//! дешевшою, але поворот гладкої сфери показати не можна взагалі — силует і
//! нормаль переходять самі в себе (R1). Камера третьої особи стежить саме за
//! орієнтацією, тож фікстура, у якої орієнтації не видно, сховала б помилку
//! так само, як камера над центром грані куба сховала D13 і D14. Ніс відрізняє
//! напрямок осі, стабілізатори — площину, ілюмінатор — крен: чотири
//! стабілізатори самі по собі лишають симетрію 90°, і ламає її саме він.
//!
//! Осі: корабель дивиться носом уздовж `+Z`, початок координат — на середині
//! між хвостом і носом.

use crate::sphere::Mesh;

/// Висота корабля-заглушки, метри. Кілька метрів — той масштаб, на якому
/// `near` зі `Scene` уперше має значення: висота над тілом, поділена на
/// десять, відсікала б корпус цілком.
pub const DEFAULT_HEIGHT_M: f64 = 6.0;

/// Найбільший радіус корпусу — частка висоти.
const RADIUS_FRACTION: f64 = 0.2;

/// Скільки граней має коло корпусу. Від нього залежить і меш, і аналітичний
/// об'єм: багатокутник має площу `(n/2)·sin(2π/n)·r²`, а не `π·r²`, і оракул
/// нижче звіряється саме з першим — тобто точно, а не з допуском на грубість
/// сітки.
const SEGMENTS: u32 = 32;

/// Скільки граней має коло ілюмінатора.
const PORTHOLE_SEGMENTS: u32 = 12;

const FINS: u32 = 4;

/// Профіль корпусу від хвоста до носа: (частка висоти, частка найбільшого
/// радіуса). Вертикальні ділянки (однакове `z`) — це кільця: сопловий комір
/// біля хвоста й уступ під носовим конусом.
const PROFILE: [[f64; 2]; 14] = [
    [0.000, 0.000],
    [0.000, 0.260],
    [0.045, 0.260],
    [0.060, 0.330],
    [0.120, 0.520],
    [0.250, 0.760],
    [0.400, 0.930],
    [0.520, 1.000],
    [0.640, 0.960],
    [0.720, 0.860],
    [0.760, 0.800],
    [0.775, 0.800],
    [0.790, 0.760],
    [1.000, 0.000],
];

/// Трикутник стабілізатора в площині (радіус, висота), у частках `r_max` і
/// висоти: корінь усередині корпусу, носок назовні й трохи нижче хвоста.
const FIN_PROFILE: [[f64; 2]; 3] = [[0.40, 0.300], [0.40, 0.020], [1.90, 0.000]];

/// Товщина стабілізатора — частка найбільшого радіуса.
const FIN_THICKNESS: f64 = 0.10;

/// Ілюмінатор: висота, внутрішній і зовнішній радіуси його осі та власний
/// радіус — усе в тих самих частках. Зовнішній кінець виступає за обвід
/// корпусу (там він ≈ 0.967), інакше кільце не було б видно збоку.
const PORTHOLE_Z: f64 = 0.620;
const PORTHOLE_INNER: f64 = 0.900;
const PORTHOLE_OUTER: f64 = 1.120;
const PORTHOLE_RADIUS: f64 = 0.220;

/// Меш корабля: корпус обертанням профілю, чотири стабілізатори й
/// ілюмінатор. Компоненти **не зшиваються** між собою — кожен є замкненою
/// оболонкою сам по собі, і саме тому знаковий об'єм усього меша дорівнює
/// сумі їхніх об'ємів навіть там, де вони перетинаються.
/// Шорсткість корпусу у фікстурах рушія й гри.
///
/// Не виміряна величина, а вибір вигляду: 0.35 — це «шліфований метал», у
/// якого відблиск уже не дзеркало, але ще й не матова пляма. Справжнє число
/// приїде з матеріалу моделі (T5d); ця стала існує рівно для того, щоб
/// зонди й тести не розводили корпус на десять різних матеріалів.
pub const HULL_ROUGHNESS: f32 = 0.35;

/// Корпус — метал, тобто дифузного відбиття не має взагалі.
///
/// Одиниця, а не 0.9: проміжних значень фізично не буває, і ставити їх
/// «щоб було м'якше» означає малювати матеріал, якого не існує.
pub const HULL_METALLIC: f32 = 1.0;

pub fn generate(height_m: f64) -> Mesh {
    let mut mesh = Mesh {
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
    };

    hull(height_m, &mut mesh);
    for k in 0..FINS {
        let angle = 2.0 * std::f64::consts::PI * f64::from(k) / f64::from(FINS);
        fin(height_m, angle, &mut mesh);
    }
    porthole(height_m, &mut mesh);

    mesh
}

/// Об'єм, який мусить огорнути меш, — сума об'ємів усіх п'яти оболонок.
/// Рахується з тих самих таблиць, що й геометрія, але **іншим шляхом**:
/// зрізаними конусами замість трикутників. Це і є оракул кроку.
pub fn volume_m3(height_m: f64) -> f64 {
    let r_max = height_m * RADIUS_FRACTION;

    let ring = ring_area(SEGMENTS);
    let mut hull = 0.0;
    for pair in PROFILE.windows(2) {
        let (z0, r0) = (pair[0][0] * height_m, pair[0][1] * r_max);
        let (z1, r1) = (pair[1][0] * height_m, pair[1][1] * r_max);
        hull += (z1 - z0) * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    }
    hull *= ring;

    let a = FIN_PROFILE[0];
    let b = FIN_PROFILE[1];
    let c = FIN_PROFILE[2];
    let area = 0.5
        * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs()
        * r_max
        * height_m;
    let fins = f64::from(FINS) * area * FIN_THICKNESS * r_max;

    let porthole = ring_area(PORTHOLE_SEGMENTS)
        * (PORTHOLE_RADIUS * r_max).powi(2)
        * (PORTHOLE_OUTER - PORTHOLE_INNER)
        * r_max;

    hull + fins + porthole
}

/// Площа правильного `n`-кутника, вписаного в коло одиничного радіуса. Для
/// нескінченного `n` це π; сітка з `n` граней огортає рівно стільки.
fn ring_area(segments: u32) -> f64 {
    let n = f64::from(segments);
    0.5 * n * (2.0 * std::f64::consts::PI / n).sin()
}

/// Корпус: профіль, обернений навколо осі `Z`. Нормаль у вузлі профілю —
/// середнє нормалей сусідніх відрізків, порахованих **у метрах**: `z` і `r`
/// масштабуються по-різному, тож у частках нахил був би не тим.
fn hull(height_m: f64, mesh: &mut Mesh) {
    let r_max = height_m * RADIUS_FRACTION;
    let base = mesh.positions.len() as u32;

    for (i, point) in PROFILE.iter().enumerate() {
        let z = (point[0] - 0.5) * height_m;
        let r = point[1] * r_max;
        let (n_r, n_z) = profile_normal(i, height_m, r_max);

        for j in 0..=SEGMENTS {
            let phi = 2.0 * std::f64::consts::PI * f64::from(j) / f64::from(SEGMENTS);
            let (sin_phi, cos_phi) = phi.sin_cos();

            mesh.positions.push([r * cos_phi, r * sin_phi, z]);
            mesh.normals
                .push([(n_r * cos_phi) as f32, (n_r * sin_phi) as f32, n_z as f32]);
        }
    }

    // +1, бо шов замикається повторенням вершини, не індексом-обгорткою —
    // так само, як у `sphere::generate`.
    let stride = SEGMENTS + 1;
    for i in 0..(PROFILE.len() as u32 - 1) {
        for j in 0..SEGMENTS {
            let a = base + i * stride + j;
            let b = a + stride;

            mesh.indices.push(a);
            mesh.indices.push(a + 1);
            mesh.indices.push(b);

            mesh.indices.push(a + 1);
            mesh.indices.push(b + 1);
            mesh.indices.push(b);
        }
    }
}

/// Нормаль у вузлі профілю, у площині (радіус, вісь). Для відрізка з
/// приростами `dz` і `dr` зовнішня нормаль — `(dz, −dr)`: на вертикальному
/// кільці (`dz = 0`) вона стає чистим `∓Z`, тобто кільце дивиться вздовж осі,
/// як і має.
fn profile_normal(i: usize, height_m: f64, r_max: f64) -> (f64, f64) {
    let segment = |a: usize, b: usize| {
        let dz = (PROFILE[b][0] - PROFILE[a][0]) * height_m;
        let dr = (PROFILE[b][1] - PROFILE[a][1]) * r_max;
        let length = (dz * dz + dr * dr).sqrt();
        (dz / length, -dr / length)
    };

    let (mut n_r, mut n_z) = (0.0, 0.0);
    if i > 0 {
        let (r, z) = segment(i - 1, i);
        n_r += r;
        n_z += z;
    }
    if i + 1 < PROFILE.len() {
        let (r, z) = segment(i, i + 1);
        n_r += r;
        n_z += z;
    }

    let length = (n_r * n_r + n_z * n_z).sqrt();
    (n_r / length, n_z / length)
}

/// Стабілізатор: трикутна призма, повернена на `angle` навколо осі. Грані
/// пласкі, тож вершини в них власні — усереднена нормаль на тонкому клині
/// вийшла б майже нульовою.
fn fin(height_m: f64, angle: f64, mesh: &mut Mesh) {
    let r_max = height_m * RADIUS_FRACTION;
    let half = 0.5 * FIN_THICKNESS * r_max;
    let (sin_a, cos_a) = angle.sin_cos();

    let corner = |k: usize, side: f64| {
        let r = FIN_PROFILE[k][0] * r_max;
        let z = (FIN_PROFILE[k][1] - 0.5) * height_m;
        [
            r * cos_a - side * half * sin_a,
            r * sin_a + side * half * cos_a,
            z,
        ]
    };

    let plus: Vec<[f64; 3]> = (0..3).map(|k| corner(k, 1.0)).collect();
    let minus: Vec<[f64; 3]> = (0..3).map(|k| corner(k, -1.0)).collect();

    // Торці. Обхід A→B→C у площині (радіус, вісь) дає нормаль проти дотичної,
    // тож бік «+» замикається зворотним порядком.
    push_triangle(mesh, plus[0], plus[2], plus[1]);
    push_triangle(mesh, minus[0], minus[1], minus[2]);

    for k in 0..3 {
        let n = (k + 1) % 3;
        push_triangle(mesh, plus[k], plus[n], minus[n]);
        push_triangle(mesh, plus[k], minus[n], minus[k]);
    }
}

/// Ілюмінатор: короткий циліндр упоперек корпусу, закритий з обох кінців.
/// Він єдиний у кораблі, тож саме він ламає симетрію 90°, яку лишають
/// чотири стабілізатори.
fn porthole(height_m: f64, mesh: &mut Mesh) {
    let r_max = height_m * RADIUS_FRACTION;
    let radius = PORTHOLE_RADIUS * r_max;
    let z = (PORTHOLE_Z - 0.5) * height_m;
    let inner = PORTHOLE_INNER * r_max;
    let outer = PORTHOLE_OUTER * r_max;

    let rim = |x: f64, k: u32| {
        let alpha = 2.0 * std::f64::consts::PI * f64::from(k) / f64::from(PORTHOLE_SEGMENTS);
        let (sin_alpha, cos_alpha) = alpha.sin_cos();
        [x, radius * cos_alpha, z + radius * sin_alpha]
    };

    let centre_inner = [inner, 0.0, z];
    let centre_outer = [outer, 0.0, z];

    for k in 0..PORTHOLE_SEGMENTS {
        let n = (k + 1) % PORTHOLE_SEGMENTS;

        push_triangle(mesh, rim(inner, k), rim(outer, n), rim(outer, k));
        push_triangle(mesh, rim(inner, k), rim(inner, n), rim(outer, n));

        push_triangle(mesh, centre_outer, rim(outer, k), rim(outer, n));
        push_triangle(mesh, centre_inner, rim(inner, n), rim(inner, k));
    }
}

/// Плаский трикутник: три власні вершини з нормаллю самої грані.
fn push_triangle(mesh: &mut Mesh, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let normal = [
        (n[0] / length) as f32,
        (n[1] / length) as f32,
        (n[2] / length) as f32,
    ];

    let base = mesh.positions.len() as u32;
    for p in [a, b, c] {
        mesh.positions.push(p);
        mesh.normals.push(normal);
    }
    mesh.indices.push(base);
    mesh.indices.push(base + 1);
    mesh.indices.push(base + 2);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere;

    /// Знаковий об'єм замкненої оболонки за теоремою про дивергенцію. Додатний
    /// — обхід зовнішній; сума по компонентах адитивна навіть при перетині,
    /// бо кожна оболонка інтегрується сама по собі.
    fn signed_volume(mesh: &Mesh) -> f64 {
        let mut total = 0.0;
        for t in mesh.indices.chunks(3) {
            let a = mesh.positions[t[0] as usize];
            let b = mesh.positions[t[1] as usize];
            let c = mesh.positions[t[2] as usize];
            total += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        total / 6.0
    }

    /// Найбільша відстань від повернутої вершини до найближчої вихідної, в
    /// обидва боки. Нуль означає, що поворот переводить меш у себе.
    fn misfit(mesh: &Mesh, turn: fn([f64; 3]) -> [f64; 3]) -> f64 {
        let turned: Vec<[f64; 3]> = mesh.positions.iter().map(|p| turn(*p)).collect();
        let nearest = |p: [f64; 3], set: &[[f64; 3]]| {
            set.iter()
                .map(|q| {
                    let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                })
                .fold(f64::INFINITY, f64::min)
        };

        let there = turned
            .iter()
            .map(|p| nearest(*p, &mesh.positions))
            .fold(0.0, f64::max);
        let back = mesh
            .positions
            .iter()
            .map(|p| nearest(*p, &turned))
            .fold(0.0, f64::max);
        there.max(back)
    }

    fn roll_90(p: [f64; 3]) -> [f64; 3] {
        [-p[1], p[0], p[2]]
    }
    fn roll_180(p: [f64; 3]) -> [f64; 3] {
        [-p[0], -p[1], p[2]]
    }
    fn end_over_end(p: [f64; 3]) -> [f64; 3] {
        [p[0], -p[1], -p[2]]
    }

    #[test]
    fn the_shells_enclose_exactly_the_volume_the_tables_say() {
        let mesh = generate(DEFAULT_HEIGHT_M);
        let measured = signed_volume(&mesh);
        let expected = volume_m3(DEFAULT_HEIGHT_M);

        // Знак додатний — усі п'ять оболонок обходяться назовні; збіг з
        // числом — вони ще й замкнені: діра або перевернутий трикутник
        // зсунули б суму.
        assert!(
            (measured - expected).abs() < 1.0e-12 * expected,
            "меш огортає {measured} м³, а таблиці кажуть {expected}"
        );
    }

    #[test]
    fn the_volume_grows_with_the_cube_of_the_height() {
        let one = volume_m3(1.0);
        let two = volume_m3(2.0);
        assert!(
            (two - 8.0 * one).abs() < 1.0e-12 * two,
            "{two} проти {} — форма залежить від висоти",
            8.0 * one
        );
    }

    #[test]
    fn no_turn_but_the_identity_leaves_the_ship_where_it_was() {
        let ship = generate(DEFAULT_HEIGHT_M);

        // Крен на 90° переставляє стабілізатори самі на себе — лишається
        // тільки ілюмінатор, і саме він мусить це зловити.
        let roll = misfit(&ship, roll_90);
        assert!(roll > 0.1, "крен на 90° лишає корабель на місці: {roll} м");
        assert!(misfit(&ship, roll_180) > 0.1);
        assert!(misfit(&ship, end_over_end) > 1.0);

        // Контроль: та сама перевірка на кулі мовчить — і саме тому куля
        // фікстурою для орієнтації бути не може.
        let ball = sphere::generate(0.5 * DEFAULT_HEIGHT_M, 16, SEGMENTS);
        assert!(
            misfit(&ball, roll_90) < 1.0e-9,
            "куля раптом несиметрична — перевірка міряє не те"
        );
    }

    #[test]
    fn every_index_is_in_range() {
        let mesh = generate(DEFAULT_HEIGHT_M);
        let count = mesh.positions.len() as u32;
        assert_eq!(mesh.indices.len() % 3, 0);
        for &i in &mesh.indices {
            assert!(i < count, "індекс {i} поза межами {count} вершин");
        }
    }
}
