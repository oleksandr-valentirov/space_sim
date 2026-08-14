//! Вузол маневру на екрані: піккінг і ручки (ROADMAP-UI.md, U4b).
//!
//! ## Піккінг — проєкцією, не рейкастом
//!
//! Камера вже вміє `to_screen`, тож «який вузол під курсором» — це порівняння
//! в пікселях. Рейкаст у сцену без буфера ідентифікаторів був би окремою
//! підсистемою заради одного маркера.
//!
//! ## Ручки по осях, а не вільне тягнення
//!
//! Це та розвилка кроку, яку вимір вирішив на користь запасного варіанту, і
//! причина геометрична, а не смакова. Тягнення довільної точки в 3D мишею
//! неоднозначне — глибину задати нема чим. Але головне інше: **осі VNB,
//! спроєктовані на екран, не ортогональні**. Якби тягнення розкладалося
//! проєкцією на всі три одразу, рух уздовж екранного `normal` міняв би й
//! `prograde` — рівно те, що перевірка кроку забороняє. З ручками ця вимога
//! виконується **за побудовою**: схопив одну вісь — рухається одна компонента.

use engine::camera::Camera;

use crate::plan::Manoeuvre;
use crate::snapshot::VesselSnapshot;

/// Довжина ручки від вузла, пікселі. Далеко — щоб не злипалися; близько —
/// щоб не тікали за край кадру на дрібних вікнах.
pub const HANDLE_PX: f32 = 60.0;

/// Наскільки близько до ручки треба клікнути, пікселі.
pub const GRAB_PX: f32 = 14.0;

/// Скільки метрів на секунду додає один піксель тягнення.
///
/// Число тут — це чутливість інструмента, а не фізика: на типовій орбіті
/// маневри — одиниці й десятки м/с, і 0.1 м/с на піксель дає повний діапазон
/// на кілька сотень пікселів руху.
pub const M_S_PER_PX: f64 = 0.1;

/// Вузол маневру, як його видно на екрані.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeOnScreen {
    /// Номер маневру в чернетці.
    pub index: usize,
    /// Де вузол, пікселі.
    pub at: [f32; 2],
    /// Куди дивляться осі VNB від вузла — **одиничні** напрямки в пікселях.
    /// Вісь, що дивиться точно в камеру, вироджується в нуль, і тоді її
    /// ручки просто немає.
    pub axes: [[f32; 2]; 3],
}

impl NodeOnScreen {
    /// Де намальована ручка осі `axis`.
    pub fn handle(&self, axis: usize) -> [f32; 2] {
        [
            self.at[0] + self.axes[axis][0] * HANDLE_PX,
            self.at[1] + self.axes[axis][1] * HANDLE_PX,
        ]
    }
}

/// Схоплена ручка: який вузол і яка його вісь.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grab {
    pub node: usize,
    pub axis: usize,
}

/// Проєктує вузли чернетки на екран.
///
/// Стан апарата в момент маневру береться з **уже порахованих семплів**
/// (правило 5: панель не пропагує). Маневр, до якого прогноз ще не дійшов,
/// вузла не має — показувати його на випадковому місці було б гірше, ніж не
/// показувати.
pub fn nodes_on_screen(
    camera: &Camera,
    fov_y: f64,
    width: u32,
    height: u32,
    vessel: &VesselSnapshot,
    manoeuvres: &[Manoeuvre],
) -> Vec<NodeOnScreen> {
    let mut nodes = Vec::new();

    for (index, manoeuvre) in manoeuvres.iter().enumerate() {
        let Some(there) = sample_at(vessel, manoeuvre.t) else {
            continue;
        };
        let Some(at) = camera.to_screen(fov_y, width, height, there.vessel_r) else {
            continue;
        };

        // Осі VNB у світі — та сама трійка, якою `Manoeuvre::dv_inertial`
        // розгортає Δv. Один базис, а не два схожі.
        let r = [
            there.vessel_r[0] - there.body_r[0],
            there.vessel_r[1] - there.body_r[1],
            there.vessel_r[2] - there.body_r[2],
        ];
        let v = [
            there.vessel_v[0] - there.body_v[0],
            there.vessel_v[1] - there.body_v[1],
            there.vessel_v[2] - there.body_v[2],
        ];
        let prograde = normalize(v);
        let normal = normalize(cross(r, v));
        let outward = cross(prograde, normal);

        // Довжина у світі, на якій вісь малюється, — така, щоб на екрані вона
        // була помітною й на низькій орбіті, і біля Місяця. Один відсоток
        // відстані до тіла: масштаб сцени сам себе й задає.
        let length = 0.01 * norm(r).max(1.0);
        let mut axes = [[0.0f32; 2]; 3];

        for (axis, direction) in [prograde, normal, outward].iter().enumerate() {
            let tip = [
                there.vessel_r[0] + direction[0] * length,
                there.vessel_r[1] + direction[1] * length,
                there.vessel_r[2] + direction[2] * length,
            ];
            let Some(tip_px) = camera.to_screen(fov_y, width, height, tip) else {
                continue;
            };
            let d = [tip_px[0] - at[0], tip_px[1] - at[1]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len > 1e-3 {
                axes[axis] = [d[0] / len, d[1] / len];
            }
        }

        nodes.push(NodeOnScreen { index, at, axes });
    }

    nodes
}

/// Яку ручку схопили, якщо схопили.
///
/// Найближча в межах [`GRAB_PX`]; ручка, що виродилась у нуль (вісь дивиться
/// в камеру), не хапається — інакше три ручки злиплися б в одній точці й
/// вибір між ними став би випадковим.
pub fn pick_handle(nodes: &[NodeOnScreen], cursor: [f32; 2]) -> Option<Grab> {
    let mut best: Option<(f32, Grab)> = None;

    for node in nodes {
        for axis in 0..3 {
            if node.axes[axis] == [0.0, 0.0] {
                continue;
            }
            let at = node.handle(axis);
            let d = [at[0] - cursor[0], at[1] - cursor[1]];
            let distance = (d[0] * d[0] + d[1] * d[1]).sqrt();

            if distance <= GRAB_PX && best.is_none_or(|(was, _)| distance < was) {
                best = Some((
                    distance,
                    Grab {
                        node: node.index,
                        axis,
                    },
                ));
            }
        }
    }

    best.map(|(_, grab)| grab)
}

/// Скільки м/с додає тягнення на `drag_px` пікселів за схоплену ручку.
///
/// Рахується **проєкція на її вісь**, тобто рух упоперек ручки не робить
/// нічого. Знак прямий: тягнеш у бік, куди дивиться вісь, — компонента росте.
pub fn drag_to_delta(node: &NodeOnScreen, axis: usize, drag_px: [f32; 2]) -> f64 {
    let a = node.axes[axis];
    let along = f64::from(a[0] * drag_px[0] + a[1] * drag_px[1]);
    along * M_S_PER_PX
}

/// Апарат і тіло відліку в один момент — усе, з чого будується базис VNB.
#[derive(Clone, Copy, Debug)]
struct At {
    vessel_r: [f64; 3],
    vessel_v: [f64; 3],
    body_r: [f64; 3],
    /// Швидкість тіла — скінченна різниця по сусідніх семплах: семпл несе
    /// лише позицію (`crate::leg`).
    body_v: [f64; 3],
}

/// Стан апарата в момент `t` — найближчий семпл, разом із тілом.
fn sample_at(vessel: &VesselSnapshot, t: f64) -> Option<At> {
    let mut best: Option<(f64, At)> = None;

    for leg in &vessel.legs {
        for (i, sample) in leg.samples.iter().enumerate() {
            let gap = (sample.state.t - t).abs();
            if best.is_some_and(|(was, _)| gap >= was) {
                continue;
            }

            let neighbour =
                leg.samples
                    .get(i + 1)
                    .or_else(|| if i > 0 { leg.samples.get(i - 1) } else { None });
            let body_v = match neighbour {
                Some(other) => {
                    let dt = other.state.t - sample.state.t;
                    if dt == 0.0 {
                        [0.0; 3]
                    } else {
                        [
                            (other.earth[0] - sample.earth[0]) / dt,
                            (other.earth[1] - sample.earth[1]) / dt,
                            (other.earth[2] - sample.earth[2]) / dt,
                        ]
                    }
                }
                None => [0.0; 3],
            };

            best = Some((
                gap,
                At {
                    vessel_r: [sample.state.r.x, sample.state.r.y, sample.state.r.z],
                    vessel_v: [sample.state.v.x, sample.state.v.y, sample.state.v.z],
                    body_r: sample.earth,
                    body_v,
                },
            ));
        }
    }

    best.map(|(_, at)| at)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    if n == 0.0 {
        [0.0; 3]
    } else {
        [a[0] / n, a[1] / n, a[2] / n]
    }
}
