//! Де рахувати перетворення обертового фрейму — на GPU у `f32` чи на CPU у
//! `f64` (ROADMAP-UI.md, U6a1).
//!
//! PROJECT.md §7 називає перетворення у вертексному шейдері «ключовим трюком»:
//! траєкторія лежить в інерціальних координатах, перемикання фрейму — вибір
//! пайплайна, без перерахунку. F6 так і зробив і виміряв, що формула на GPU
//! збігається з C-оракулом.
//!
//! Але той самий §7 має рішення 1: **світові координати ніколи не в `float`**.
//! А перетворення на GPU вимагає саме їх — у F6 вершина несе геоцентричні
//! `vessel − earth` і `moon − earth`, до 4·10⁸ м, у `f32`. У F6 це не боліло,
//! бо камера там нерухома й далека; інтерактивна камера наближається до
//! апарата, і питання стає кількісним.
//!
//! Тому тут два числа, а не думка:
//!
//! 1. **Скільки метрів коштує `f32`-шлях.** Та сама формула проганяється
//!    двічі — у `f64` з точних чисел і у `f32` з округлених, як її побачив би
//!    шейдер, — і різниця переводиться в метри й у пікселі при кількох
//!    ширинах вигляду.
//! 2. **Скільки коштує `f64`-шлях на CPU.** Прохід camera-relative по тих
//!    самих точках уже є в кадрі щокадру (`frame::Lines::upload`); питання
//!    лише в тому, скільки додає до нього перетворення фрейму. Два числа з
//!    одного прогону, як завжди: одне без другого нічого не означає.

use std::time::Instant;

use crate::camera::Camera;
use crate::trajectory::{self, Sample, MU};

/// Ширини вигляду, для яких помилка переводиться в пікселі.
///
/// 10 км — апарат зблизька, 10⁶ км — уся система Земля-Місяць у кадрі. Між
/// ними той масштаб, на якому дивляться на орбіту біля Місяця.
const VIEW_WIDTHS_M: [f64; 4] = [1.0e4, 1.0e5, 1.0e6, 1.0e9];

/// Скільки пікселів завширшки кадр, у якому міряються пікселі помилки.
const WIDTH_PX: f64 = 1280.0;

/// Синодична позиція у `f64` — те саме, що [`trajectory::rotating_position`],
/// але з явними аргументами, щоб поруч стояла `f32`-копія.
fn rotating_f64(vessel: [f64; 3], moon: [f64; 3], z_axis: [f64; 3]) -> [f64; 3] {
    // Геоцентрично: Земля вже віднята з обох (як у вершинних даних F6).
    let d = moon;
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let x = [d[0] / length, d[1] / length, d[2] / length];
    let y = [
        z_axis[1] * x[2] - z_axis[2] * x[1],
        z_axis[2] * x[0] - z_axis[0] * x[2],
        z_axis[0] * x[1] - z_axis[1] * x[0],
    ];
    let origin = [MU * d[0], MU * d[1], MU * d[2]];
    let rel = [
        vessel[0] - origin[0],
        vessel[1] - origin[1],
        vessel[2] - origin[2],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [
        dot(rel, x) / length,
        dot(rel, y) / length,
        dot(rel, z_axis) / length,
    ]
}

/// Та сама формула так, як її бачить вершинний шейдер: входи округлені до
/// `f32`, уся арифметика у `f32`.
///
/// Це не «модель шейдера», а буквально він: `trajectory.slang` рахує
/// `synodic_basis` і проєкцію в тих самих операціях і в тому ж порядку.
fn rotating_f32(vessel: [f64; 3], moon: [f64; 3], z_axis: [f64; 3]) -> [f64; 3] {
    let narrow = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
    let (vessel, d, z_axis) = (narrow(vessel), narrow(moon), narrow(z_axis));

    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let x = [d[0] / length, d[1] / length, d[2] / length];
    let y = [
        z_axis[1] * x[2] - z_axis[2] * x[1],
        z_axis[2] * x[0] - z_axis[0] * x[2],
        z_axis[0] * x[1] - z_axis[1] * x[0],
    ];
    let mu = MU as f32;
    let origin = [mu * d[0], mu * d[1], mu * d[2]];
    let rel = [
        vessel[0] - origin[0],
        vessel[1] - origin[1],
        vessel[2] - origin[2],
    ];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [
        f64::from(dot(rel, x) / length),
        f64::from(dot(rel, y) / length),
        f64::from(dot(rel, z_axis) / length),
    ]
}

pub struct Precision {
    /// Найгірша розбіжність між двома шляхами, метри.
    pub worst_m: f64,
    /// На якому семплі вона трапилась і як далеко там був апарат від Землі.
    pub worst_sample: usize,
    pub worst_geocentric_m: f64,
    /// Середня розбіжність, метри — щоб було видно, що найгірше не викид.
    pub mean_m: f64,
    /// Масштаб `L` на найгіршому семплі, метри: у ньому синодичні одиниці.
    pub length_m: f64,
}

/// Скільки метрів коштує `f32`-шлях на всій фікстурній орбіті.
pub fn precision(samples: &[Sample]) -> Precision {
    let mut worst_m = 0.0;
    let mut worst_sample = 0;
    let mut worst_geocentric_m = 0.0;
    let mut length_m = 0.0;
    let mut total = 0.0;

    for (index, s) in samples.iter().enumerate() {
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let vessel = sub(s.vessel, s.earth);
        let moon = sub(s.moon, s.earth);
        let length = (moon[0] * moon[0] + moon[1] * moon[1] + moon[2] * moon[2]).sqrt();

        let exact = rotating_f64(vessel, moon, s.z_axis);
        let narrow = rotating_f32(vessel, moon, s.z_axis);

        // Синодичні одиниці безрозмірні — у метри їх переводить той самий
        // масштаб L, яким їх поділили.
        let error_m = length
            * ((exact[0] - narrow[0]).powi(2)
                + (exact[1] - narrow[1]).powi(2)
                + (exact[2] - narrow[2]).powi(2))
            .sqrt();

        total += error_m;
        if error_m > worst_m {
            worst_m = error_m;
            worst_sample = index;
            worst_geocentric_m =
                (vessel[0] * vessel[0] + vessel[1] * vessel[1] + vessel[2] * vessel[2]).sqrt();
            length_m = length;
        }
    }

    Precision {
        worst_m,
        worst_sample,
        worst_geocentric_m,
        mean_m: total / samples.len() as f64,
        length_m,
    }
}

/// Скільки пікселів становить `error_m` у кадрі завширшки `view_m`.
pub fn error_px(error_m: f64, view_m: f64) -> f64 {
    error_m / view_m * WIDTH_PX
}

pub struct Cost {
    pub points: usize,
    /// Прохід, який кадр робить уже сьогодні: camera-relative у `f64`.
    pub camera_ns: f64,
    /// Він же плюс перетворення фрейму — те, чого крок вимагає від CPU.
    pub camera_and_frame_ns: f64,
}

impl Cost {
    /// На скільки відсотків дорожчає прохід.
    pub fn overhead(&self) -> f64 {
        (self.camera_and_frame_ns - self.camera_ns) / self.camera_ns * 100.0
    }
}

/// Ціна обох проходів по одних і тих самих точках, наносекунди на точку.
///
/// Міряються **разом і в одному прогоні**: різниця між прогонами на одній
/// машині більша за те, що коштує перетворення.
pub fn cost(samples: &[Sample], passes: u32) -> Cost {
    let camera = Camera::look_at([4.0e8, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let points: Vec<[f64; 3]> = samples.iter().map(|s| sub(s.vessel, s.earth)).collect();
    let frames: Vec<([f64; 3], [f64; 3])> = samples
        .iter()
        .map(|s| (sub(s.moon, s.earth), s.z_axis))
        .collect();

    let mut bytes: Vec<u8> = Vec::with_capacity(points.len() * 12);

    let mut plain = || {
        bytes.clear();
        for &p in &points {
            for value in camera.relative(p) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    };
    for _ in 0..2 {
        plain();
    }
    let start = Instant::now();
    for _ in 0..passes {
        plain();
    }
    let camera_ns =
        start.elapsed().as_secs_f64() * 1.0e9 / (f64::from(passes) * points.len() as f64);
    assert_eq!(bytes.len(), points.len() * 12);

    let mut with_frame = || {
        bytes.clear();
        for (&p, &(moon, z_axis)) in points.iter().zip(frames.iter()) {
            let turned = rotating_f64(p, moon, z_axis);
            for value in camera.relative(turned) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    };
    for _ in 0..2 {
        with_frame();
    }
    let start = Instant::now();
    for _ in 0..passes {
        with_frame();
    }
    let camera_and_frame_ns =
        start.elapsed().as_secs_f64() * 1.0e9 / (f64::from(passes) * points.len() as f64);
    assert_eq!(bytes.len(), points.len() * 12);

    Cost {
        points: points.len(),
        camera_ns,
        camera_and_frame_ns,
    }
}

/// Обидва числа з одного прогону — те, що друкує `--rotating-probe`.
pub fn report() {
    let samples = trajectory::load();

    let p = precision(&samples);
    println!(
        "Точність. Та сама формула у f64 і у f32 (як її бачить вершинний шейдер),\n\
         {} семплів halo-орбіти з фікстури.\n",
        samples.len()
    );
    println!(
        "  найгірша розбіжність: {:.2} м (семпл {}, апарат за {:.3e} м від Землі, L = {:.3e} м)",
        p.worst_m, p.worst_sample, p.worst_geocentric_m, p.length_m
    );
    println!("  середня розбіжність:  {:.2} м\n", p.mean_m);

    println!(
        "  {:>14} {:>12} {:>12}",
        "ширина кадру", "м на піксель", "помилка, px"
    );
    for view in VIEW_WIDTHS_M {
        println!(
            "  {:>14.0e} {:>12.1} {:>12.2}",
            view,
            view / WIDTH_PX,
            error_px(p.worst_m, view)
        );
    }

    let c = cost(&samples, 200);
    println!(
        "\nЦіна на CPU, {} точок, наносекунди на точку (обидва числа — один прогін):\n",
        c.points
    );
    println!("  camera-relative, як зараз:        {:.2} нс", c.camera_ns);
    println!(
        "  плюс перетворення фрейму:         {:.2} нс  ({:+.0}%)",
        c.camera_and_frame_ns,
        c.overhead()
    );
}
