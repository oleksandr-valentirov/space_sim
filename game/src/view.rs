//! Снапшот → сцена (ROADMAP J1).
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

use engine::camera::Camera;
use engine::scene::{Polyline, Scene};

use crate::snapshot::WorldSnapshot;

/// Колір прогнозу — той самий, яким H5 малював живу траєкторію.
const PREDICTION: [f32; 4] = [0.9, 0.6, 0.2, 1.0];

pub fn build(snapshot: &WorldSnapshot, camera: Camera) -> Scene {
    let mut scene = Scene::new(camera);

    for vessel in &snapshot.vessels {
        let mut points = Vec::with_capacity(vessel.sample_count());

        for leg in &vessel.legs {
            for sample in &leg.samples {
                points.push([
                    sample.state.r.x - sample.earth[0],
                    sample.state.r.y - sample.earth[1],
                    sample.state.r.z - sample.earth[2],
                ]);
            }
        }

        // Ламана з однієї вершини — не ламана. Рушій такий випадок і сам
        // пропустить, але порожній `Polyline` у сцені змусив би читача
        // здогадуватися, чому він там.
        if points.len() >= 2 {
            scene.polylines.push(Polyline {
                points,
                colour: PREDICTION,
            });
        }
    }

    scene
}
