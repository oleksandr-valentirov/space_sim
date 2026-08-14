//! Снапшот → сцена (ROADMAP J1, J2).
//!
//! Уся межа між грою й рушієм в одному напрямку: тут із того, що гра знає про
//! світ, лишається те, що рушію треба намалювати. Назад не йде нічого.
//!
//! ## Чому геоцентрично
//!
//! Сфера в кадрі — в початку координат і радіуса Землі (`engine::frame`), тож
//! ламана мусить приїхати в тій самій системі: `апарат − Земля` в момент
//! кожного семпла. Це не спрощення й не тимчасовий фрейм — це та сама
//! прив'язка, що в `trajectory_render` з F6, тільки віднімання робиться тут, у
//! `double`, а не в шейдері.
//!
//! Обертовий фрейм (PROJECT.md §7 вимагає його дефолтом для карти) приїде
//! разом із сервісом фреймів; семпли для нього вже несуть позицію Місяця.
//!
//! ## Історія й прогноз — одні й ті самі ланки
//!
//! Курсор ділить їх кольором, і більше нічим: перерахунку немає, копіювання
//! немає, у сторі нічого не рухається. Саме це й означає правило 5 з
//! PROJECT.md §4 — «пораховану ділянку прогнозу час перетворює на історію».
//! Тут це видно буквально: змінюється лише те, з чим порівнюють `sample.t`.

use engine::camera::Camera;
use engine::scene::{Polyline, Scene};

use crate::snapshot::WorldSnapshot;

/// Прогноз — той самий колір, яким H5 малював живу траєкторію.
const PREDICTION: [f32; 4] = [0.9, 0.6, 0.2, 1.0];
/// Історія — приглушено, щоб було видно, куди рухається межа.
const HISTORY: [f32; 4] = [0.35, 0.45, 0.6, 1.0];
/// Маркер апарата.
const VESSEL: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Спекулятивне прев'ю з планувальника — те, чого ще не вирішили летіти.
const PREVIEW: [f32; 4] = [0.4, 0.9, 0.5, 1.0];

/// Півдовжина хреста-маркера як частка відстані до камери.
///
/// Частка, а не метри: апарат розглядають і з мільярда метрів, і зблизька, а
/// маркер має лишатися маркером — того самого розміру на екрані.
const MARKER_FRACTION: f64 = 0.01;

pub fn build(snapshot: &WorldSnapshot, camera: Camera) -> Scene {
    build_with_preview(snapshot, camera, &[])
}

/// Те саме, плюс спекулятивна лінія з планувальника (ROADMAP J5).
///
/// Прев'ю малюється окремим кольором і **поверх** прогнозу, а не замість
/// нього: гравець має бачити обидві лінії одночасно — ту, якою полетить зараз,
/// і ту, якою полетів би за новим планом.
pub fn build_with_preview(
    snapshot: &WorldSnapshot,
    camera: Camera,
    preview: &[std::sync::Arc<crate::leg::Leg>],
) -> Scene {
    let mut scene = Scene::new(camera);

    for vessel in &snapshot.vessels {
        let mut history: Vec<[f64; 3]> = Vec::new();
        let mut future: Vec<[f64; 3]> = Vec::new();

        for leg in &vessel.legs {
            for sample in &leg.samples {
                let point = [
                    sample.state.r.x - sample.earth[0],
                    sample.state.r.y - sample.earth[1],
                    sample.state.r.z - sample.earth[2],
                ];

                if sample.state.t <= snapshot.t {
                    history.push(point);
                } else {
                    // Перша точка прогнозу повторює останню точку історії,
                    // інакше між двома ламаними був би розрив завширшки в
                    // крок інтегратора — тобто в години польоту.
                    if future.is_empty() {
                        if let Some(&last) = history.last() {
                            future.push(last);
                        }
                    }
                    future.push(point);
                }
            }
        }

        push_line(&mut scene, history, HISTORY);
        push_line(&mut scene, future, PREDICTION);

        // Де апарат зараз. Позиція інтерпольована (снапшот), а Земля береться
        // з найближчого семпла: за крок інтегратора вона зсувається на частки
        // відсотка масштабу кадру, і шукати її точніше означало б четвертий
        // виклик ефемериди на кадр заради невидимого.
        if let Some(earth) = earth_near(vessel, snapshot.t) {
            let position = [
                vessel.state.r.x - earth[0],
                vessel.state.r.y - earth[1],
                vessel.state.r.z - earth[2],
            ];
            push_marker(&mut scene, position);
        }
    }

    let mut speculative = Vec::new();
    for leg in preview {
        for sample in &leg.samples {
            speculative.push([
                sample.state.r.x - sample.earth[0],
                sample.state.r.y - sample.earth[1],
                sample.state.r.z - sample.earth[2],
            ]);
        }
    }
    push_line(&mut scene, speculative, PREVIEW);

    scene
}

fn push_line(scene: &mut Scene, points: Vec<[f64; 3]>, colour: [f32; 4]) {
    // Ламана з однієї вершини — не ламана. Рушій такий випадок і сам
    // пропустить, але порожній `Polyline` у сцені змусив би читача
    // здогадуватися, чому він там.
    if points.len() >= 2 {
        scene.polylines.push(Polyline { points, colour });
    }
}

/// Позиція Землі в семплі, найближчому до `t`.
fn earth_near(vessel: &crate::snapshot::VesselSnapshot, t: f64) -> Option<[f64; 3]> {
    let mut best: Option<(f64, [f64; 3])> = None;

    for leg in &vessel.legs {
        for sample in &leg.samples {
            let gap = (sample.state.t - t).abs();
            if best.is_none_or(|(was, _)| gap < was) {
                best = Some((gap, sample.earth));
            }
        }
    }

    best.map(|(_, earth)| earth)
}

/// Хрест із трьох відрізків у точці.
///
/// Три ламані, а не точка: `PointList` дав би один піксель, який не видно, а
/// власного примітиву для маркерів рушій не має й не мусить мати заради
/// цього.
fn push_marker(scene: &mut Scene, position: [f64; 3]) {
    let camera = scene.camera.position();
    let distance = {
        let d = [
            position[0] - camera[0],
            position[1] - camera[1],
            position[2] - camera[2],
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let arm = distance * MARKER_FRACTION;

    for axis in 0..3 {
        let mut a = position;
        let mut b = position;
        a[axis] -= arm;
        b[axis] += arm;
        scene.polylines.push(Polyline {
            points: vec![a, b],
            colour: VESSEL,
        });
    }
}
