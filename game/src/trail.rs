//! Кеш прорідженого сліду на ланку (ROADMAP.md, N2b).
//!
//! N2a показав, що критерій у пікселях коштує 415 мс на кадр — у сорок разів
//! більше за 9 мс, які він економить. Тобто проріджування має сенс лише тоді,
//! коли рахується **не щокадру**, і саме це робить цей модуль.
//!
//! ## На чому кеш узагалі тримається
//!
//! **Точка семпла в кадрі не залежить від часу кадру — ні в інерціальному
//! фреймі, ні в обертовому.** Для інерціального це очевидно: `апарат − Земля`
//! в мить семпла. Для обертового — ні, і це варто сказати прямо: `Synodic`
//! семпла будується з його **власної** лінії Земля-Місяць і власної нормалі, а
//! з кадру бере лише `scale` (стала `SYNODIC_SCALE_M`) і `mass_ratio` (з
//! ассета). Отже позиція семпла в синодичних координатах — стала від моменту,
//! коли семпл порахували, і кешувати її можна назавжди, а не до наступного
//! кадру. Перевіряє це `game/tests/trail.rs`, а не цей коментар.
//!
//! ## Чому допуск у метрах, хоч критерій екранний
//!
//! N2a рахував відхилення у **пікселях**, тобто результат залежав від того,
//! звідки камера дивиться, — і при повороті камери кеш довелося б викидати.
//! Тут інакше: відхилення міряється в метрах, а допуск виводиться з відстані —
//! `tol_px · d / focal_px`, де `d` — **найближча** точка ланки до камери.
//! Просторове відхилення ніколи не менше за екранне, тож такий допуск
//! консервативний: він може лишити зайву вершину, але не може прибрати видиму.
//! Натомість він не залежить від напрямку погляду, тож кеш переживає обертання
//! камери — а обертає її гравець постійно.
//!
//! ## Що інвалідує запис
//!
//! Ланка незмінна від моменту, коли її порахували, тож інвалідує лише зміна
//! масштабу — і не будь-яка, а перехід через степінь двійки. Ключ — адреса
//! `Arc<Leg>`, і запис **тримає той самий `Arc`**: інакше звільнена ланка
//! віддала б свою адресу новій, і кеш тихо відповів би чужими точками.

use std::collections::HashMap;
use std::sync::Arc;

use engine::camera::Camera;

use crate::frame_view::{Synodic, ViewFrame};
use crate::leg::Leg;

/// Точка сліду: час семпла й позиція в тому фреймі, у якому малюють.
///
/// Час потрібен, бо курсор ділить слід на історію й прогноз, а після
/// проріджування семпла за індексом уже не знайти.
pub type Point = (f64, [f64; 3]);

struct Entry {
    /// Сама ланка — щоб її адреса, яка є ключем, лишалася зайнятою.
    leg: Arc<Leg>,
    frame: ViewFrame,
    /// Показник степеня двійки, у якому лежить допуск у метрах.
    bucket: i32,
    /// Центр і радіус ланки в цьому фреймі — сталі, як і самі точки.
    centre: [f64; 3],
    radius: f64,
    points: Vec<Point>,
    /// Номер кадру, на якому запис востаннє знадобився.
    used: u64,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<usize, Entry>,
    frame: u64,
}

impl Cache {
    pub fn new() -> Cache {
        Cache::default()
    }

    /// Скільки ланок лежить у кеші. Для тестів і зонда.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Новий кадр: далі `points` позначатиме потрібне саме йому.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Викинути те, чого цей кадр не питав.
    ///
    /// Без цього кеш тримав би ланки, яких у світі вже немає, — а після правки
    /// плану каскад викидає їх десятками (J3).
    pub fn sweep(&mut self) {
        let frame = self.frame;
        self.entries.retain(|_, entry| entry.used == frame);
    }

    /// Проріджені точки ланки в заданому фреймі.
    ///
    /// `synodic` — базис «зараз»; з нього беруться лише масштаб і `μ`, обидва
    /// сталі (див. вступ модуля). `None` означає інерціальний фрейм.
    pub fn points(
        &mut self,
        leg: &Arc<Leg>,
        frame: ViewFrame,
        synodic: Option<&Synodic>,
        camera: &Camera,
        focal_px: f64,
        tol_px: f64,
    ) -> &[Point] {
        let key = Arc::as_ptr(leg) as usize;
        let now = self.frame;

        // Габарит потрібен, щоб узнати масштаб, а масштаб — щоб узнати, чи
        // годиться запис. Тож перший прохід рахує габарит, якщо запису ще
        // немає або він з іншого фрейму.
        let fresh = match self.entries.get(&key) {
            Some(entry) => Arc::ptr_eq(&entry.leg, leg) && entry.frame == frame,
            None => false,
        };

        if !fresh {
            let points = transform(leg, synodic);
            let (centre, radius) = bounds(&points);
            let bucket = bucket_of(tolerance_m(camera, centre, radius, focal_px, tol_px));
            self.entries.insert(
                key,
                Entry {
                    leg: leg.clone(),
                    frame,
                    bucket,
                    centre,
                    radius,
                    points: thin(&points, exponent_to_metres(bucket)),
                    used: now,
                },
            );
            return &self.entries[&key].points;
        }

        let entry = self.entries.get_mut(&key).expect("щойно перевірили");
        entry.used = now;

        let bucket = bucket_of(tolerance_m(
            camera,
            entry.centre,
            entry.radius,
            focal_px,
            tol_px,
        ));
        if bucket != entry.bucket {
            let points = transform(&entry.leg, synodic);
            entry.points = thin(&points, exponent_to_metres(bucket));
            entry.bucket = bucket;
        }

        &entry.points
    }
}

/// Точки ланки у фреймі, у якому їх малюють.
fn transform(leg: &Leg, synodic: Option<&Synodic>) -> Vec<Point> {
    let normals = crate::view::plane_normals(&leg.samples);
    let mut out = Vec::with_capacity(leg.samples.len());
    for (index, sample) in leg.samples.iter().enumerate() {
        let point = match synodic {
            None => crate::view::geocentric(sample),
            Some(now) => match crate::view::sample_frame(sample, normals[index], now) {
                Some(turned) => turned,
                None => continue,
            },
        };
        out.push((sample.state.t, point));
    }
    out
}

fn bounds(points: &[Point]) -> ([f64; 3], f64) {
    if points.is_empty() {
        return ([0.0; 3], 0.0);
    }

    let mut lo = points[0].1;
    let mut hi = points[0].1;
    for (_, p) in points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }

    let centre = [
        (lo[0] + hi[0]) * 0.5,
        (lo[1] + hi[1]) * 0.5,
        (lo[2] + hi[2]) * 0.5,
    ];
    let half = [
        (hi[0] - lo[0]) * 0.5,
        (hi[1] - lo[1]) * 0.5,
        (hi[2] - lo[2]) * 0.5,
    ];
    let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
    (centre, radius)
}

/// Допуск у метрах для ланки, найближча точка якої за `d` від камери.
///
/// `d` береться з габаритної сфери й ніколи не менший за невелику частку
/// радіуса: камера **всередині** сфери ланки — це не «нуль метрів на піксель»,
/// а той самий випадок, який двічі коштував кадру в патчах (D13, D14).
fn tolerance_m(camera: &Camera, centre: [f64; 3], radius: f64, focal_px: f64, tol_px: f64) -> f64 {
    let eye = camera.position();
    let d = [centre[0] - eye[0], centre[1] - eye[1], centre[2] - eye[2]];
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - radius;
    // Всередині габариту найближча точка може бути під самим оком; там
    // проріджувати не можна взагалі, і сота частка радіуса — це «майже
    // нічого», а не нуль, з якого вийшов би допуск нуль і жодної економії.
    let distance = distance.max(radius * 0.01).max(1.0);
    tol_px * distance / focal_px
}

/// Степінь двійки, у якому лежить допуск.
fn bucket_of(tolerance_m: f64) -> i32 {
    tolerance_m.log2().floor() as i32
}

/// Нижня межа кошика — саме її беруть за допуск.
///
/// Нижня, а не середина: кошик має бути **не суворішим** за те, що просила
/// камера, лише коли це безпечно. Беручи нижню межу, ми завжди прорідили
/// менше, ніж дозволено, і жодна вершина не зникає раніше часу.
fn exponent_to_metres(bucket: i32) -> f64 {
    (bucket as f64).exp2()
}

/// Дуглас-Пекер у метрах, у просторі кадру.
fn thin(points: &[Point], tol_m: f64) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let plane: Vec<[f64; 3]> = points.iter().map(|&(_, p)| p).collect();
    crate::thin::simplify3(&plane, tol_m)
        .into_iter()
        .map(|index| points[index])
        .collect()
}
