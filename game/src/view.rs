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
use engine::scene::{Body, Polyline, Scene, TerrainId, TileSet};

use crate::frame_view::{self, Synodic, ViewFrame};
use crate::snapshot::WorldSnapshot;
use crate::world::{EARTH, MOON};

// Кольори ліній переїхали в `palette` (U7c) — не заради порядку, а тому що
// саме вони й задають палітру інтерфейсу: акцент панелі зобов'язаний бути
// кольором прогнозу, і живучи в двох місцях, ці двоє тихо розійшлися б.
//
// Числа при переїзді не змінились: [0.9, 0.6, 0.2] — це (229, 153, 51) у тих
// самих одиницях, бо ціль кадру не sRGB і байт ділиться на 255 без гамми.
// Перевіряють це тести `palette`, а не коментар.
use crate::palette;

/// Півдовжина хреста-маркера як частка відстані до камери.
///
/// Частка, а не метри: апарат розглядають і з мільярда метрів, і зблизька, а
/// маркер має лишатися маркером — того самого розміру на екрані.
const MARKER_FRACTION: f64 = 0.01;

/// Розмір кадру в пікселях — усе, що проріджуванню треба знати про вікно.
///
/// Не `engine::ui::Viewport`: той несе ще й масштаб інтерфейсу й позицію
/// курсора, а тут потрібні рівно два числа. Ширина потрібна разом із висотою,
/// бо горизонтальний кут зору виводиться зі співвідношення сторін, і без неї
/// відхилення по `x` міряли б у інших пікселях, ніж по `y`.
#[derive(Clone, Copy)]
pub struct Viewport {
    pub width_px: u32,
    pub height_px: u32,
}

pub fn build(snapshot: &WorldSnapshot, camera: Camera) -> Scene {
    build_with_preview(snapshot, camera, &[], ViewFrame::Inertial)
}

/// Те саме в заданому фреймі (ROADMAP-UI.md, U6a2).
pub fn build_in(snapshot: &WorldSnapshot, camera: Camera, frame: ViewFrame) -> Scene {
    build_with_preview(snapshot, camera, &[], frame)
}

/// Те саме, але сліди проріджені за екранним критерієм (N2a).
///
/// Окремим входом, а не прапорцем у [`build_in`], і не заради сумісності:
/// оракул кроку — це порівняння **двох** сцен, проріджена проти повної, тож
/// обидві мусять будуватися однаково легко. Гра кличе цю, тести — обидві.
pub fn build_thinned(
    snapshot: &WorldSnapshot,
    camera: Camera,
    preview: &[std::sync::Arc<crate::leg::Leg>],
    frame: ViewFrame,
    viewport: Viewport,
) -> Scene {
    build_all(snapshot, camera, preview, frame, Some(viewport))
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
    frame: ViewFrame,
) -> Scene {
    build_all(snapshot, camera, preview, frame, None)
}

/// Спільне тіло обох входів: `thin` вирішує, чи проріджувати сліди.
///
/// `None` означає «віддати кожен семпл», і саме таким сцену бачив увесь код до
/// N2a — тобто це не «вимкнена оптимізація», а еталон, проти якого міряється
/// проріджений.
fn build_all(
    snapshot: &WorldSnapshot,
    camera: Camera,
    preview: &[std::sync::Arc<crate::leg::Leg>],
    frame: ViewFrame,
    thin: Option<Viewport>,
) -> Scene {
    let mut scene = Scene::new(camera);

    // Базис «зараз» — для тіл і маркерів. Точки траєкторії беруть базис своєї
    // миті, і саме тому він рахується не тут, а поруч із семплом.
    //
    // Немає базису — немає й обертового фрейму: якщо в ассеті немає Місяця
    // або він стоїть в одній точці із Землею, сцена лишається інерціальною.
    // Це не тихе ігнорування вибору: інерціальний кадр — правильна відповідь
    // на «пари тіл немає», а NaN у кожній вершині — ні.
    let now = match frame {
        ViewFrame::Inertial => None,
        ViewFrame::Rotating => synodic_now(snapshot),
    };
    let moon_now = moon_local(snapshot).unwrap_or([0.0; 3]);

    // Тіла — те саме віднімання Землі, що й для ламаних, і з тієї ж причини:
    // кадр геоцентричний (див. вступ модуля). Земля опиняється рівно в
    // початку координат, Місяць — там, де він відносно неї в цю мить, а в
    // обертовому фреймі обидва стоять нерухомо.
    if let Some(earth) = snapshot.bodies.iter().find(|b| b.body == EARTH) {
        for body in &snapshot.bodies {
            // Тіло без розміру малювати нема як: радіус нуль — це «ассет не
            // каже», а не «крапка».
            if body.radius_m <= 0.0 {
                continue;
            }
            let centre = [
                body.position[0] - earth.position[0],
                body.position[1] - earth.position[1],
                body.position[2] - earth.position[2],
            ];
            scene.bodies.push(Body {
                centre: match now {
                    Some(s) => s.apply(centre, moon_now),
                    None => centre,
                },
                radius_m: body.radius_m,
                // Поворот тіла навколо власного центра від вибору початку
                // координат не залежить — а от від вибору **осей** залежить,
                // і в обертовому фреймі осі інші.
                orientation: match now {
                    Some(s) => frame_view::compose(s.rotation(), body.orientation),
                    None => body.orientation,
                },
                // Гладке за замовчуванням; рельєф вмикає `attach_terrain`
                // після побудови, бо хендл тайлів видає кадр, а не снапшот
                // (D12).
                tiles: TileSet::Smooth,
            });
        }
    }

    // Крива нульової швидкості — тільки в обертовому фреймі, і це не
    // обмеження реалізації: вона живе в площині синодичної системи й в
    // інерціальному кадрі оберталася б разом із парою, показуючи стіну там,
    // де її щойно не було.
    if now.is_some() {
        if let (Some(mu), Some(c)) = (mass_ratio(snapshot), current_jacobi(snapshot)) {
            scene
                .polylines
                .extend(crate::zvc::curves(mu, c, frame_view::SYNODIC_SCALE_M));
        }
    }

    for vessel in &snapshot.vessels {
        let mut history: Vec<[f64; 3]> = Vec::new();
        let mut future: Vec<[f64; 3]> = Vec::new();

        for leg in &vessel.legs {
            let normals = plane_normals(&leg.samples);
            for (index, sample) in leg.samples.iter().enumerate() {
                let point = geocentric(sample);
                // Кожна точка бере базис **своєї миті** — у цьому вся суть
                // обертового фрейму: базис «зараз» дав би просто повернуту
                // інерціальну траєкторію.
                let point = match now {
                    Some(s) => match sample_frame(sample, normals[index], &s) {
                        Some(turned) => turned,
                        // Виродженого базису на семплі бути не може, якщо він
                        // є «зараз», — але мовчазний NaN коштував би дорожче
                        // за цю гілку.
                        None => continue,
                    },
                    None => point,
                };

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

        push_trail(&mut scene, history, palette::HISTORY.scene(), thin);
        push_trail(&mut scene, future, palette::PREDICTION.scene(), thin);

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
            // Маркер — це «зараз», тож і базис у нього теперішній.
            let position = match now {
                Some(s) => s.apply(position, moon_now),
                None => position,
            };
            push_marker(&mut scene, position);
        }
    }

    let mut speculative = Vec::new();
    for leg in preview {
        let normals = plane_normals(&leg.samples);
        for (index, sample) in leg.samples.iter().enumerate() {
            match now {
                Some(s) => {
                    if let Some(turned) = sample_frame(sample, normals[index], &s) {
                        speculative.push(turned);
                    }
                }
                None => speculative.push(geocentric(sample)),
            }
        }
    }
    push_trail(&mut scene, speculative, palette::PREVIEW.scene(), thin);

    scene
}

/// Ламана сліду: та сама [`push_line`], але через критерій N2a, якщо просили.
///
/// Проріджуються **сліди**, а не всі ламані сцени. Хрест маркера — три
/// відрізки по дві точки, і критерій над ними або нічого не зробить, або
/// з'їсть маркер; крива нульової швидкості будується не з семплів, і в неї
/// своя ціна (борг D11).
fn push_trail(scene: &mut Scene, points: Vec<[f64; 3]>, colour: [f32; 4], thin: Option<Viewport>) {
    let points = match thin {
        None => points,
        Some(viewport) => {
            let kept = crate::thin::keep(
                &points,
                &scene.camera,
                engine::frame::FOV_Y,
                viewport.width_px,
                viewport.height_px,
                crate::thin::TOLERANCE_PX,
            );
            kept.into_iter().map(|index| points[index]).collect()
        }
    };
    push_line(scene, points, colour);
}

/// Позиція семпла відносно Землі **тієї самої миті** — те, з чого починається
/// будь-який із двох фреймів.
fn geocentric(sample: &crate::leg::Sample) -> [f64; 3] {
    [
        sample.state.r.x - sample.earth[0],
        sample.state.r.y - sample.earth[1],
        sample.state.r.z - sample.earth[2],
    ]
}

/// Точка семпла в синодичному фреймі його власної миті, у масштабі `now`.
fn sample_frame(sample: &crate::leg::Sample, normal: [f64; 3], now: &Synodic) -> Option<[f64; 3]> {
    let d = [
        sample.moon[0] - sample.earth[0],
        sample.moon[1] - sample.earth[1],
        sample.moon[2] - sample.earth[2],
    ];
    let basis = now.with_line(d, normal)?;
    Some(basis.apply(geocentric(sample), d))
}

/// Нормалі миттєвої площини Земля-Місяць по семплах однієї ланки.
///
/// Публічна заради тесту, який звіряє **перетворення** з формулою рушія
/// (`engine::trajectory::rotating_position`, звіреною з C-оракулом): якби тест
/// рахував нормалі по-своєму, він порівнював би дві різні площини й списував
/// би розбіжність на них.
///
/// Центральна різниця, а не аналітична швидкість Місяця: у семплі її немає й
/// не буде — 104 байти на семпл це вже борг D7, і додавати до них ще 24 заради
/// вигляду не варто. F6 виміряв, що при кроці ~2.7 год центральна різниця дає
/// розбіжність 3.5·10⁻⁷ проти C-оракула; на краях ланки різниця однобічна.
pub fn plane_normals(samples: &[crate::leg::Sample]) -> Vec<[f64; 3]> {
    let line = |s: &crate::leg::Sample| {
        [
            s.moon[0] - s.earth[0],
            s.moon[1] - s.earth[1],
            s.moon[2] - s.earth[2],
        ]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };

    (0..samples.len())
        .map(|i| {
            let before = line(&samples[i.saturating_sub(1)]);
            let after = line(&samples[(i + 1).min(samples.len() - 1)]);
            let rate = [
                after[0] - before[0],
                after[1] - before[1],
                after[2] - before[2],
            ];
            cross(line(&samples[i]), rate)
        })
        .collect()
}

/// Частка маси пари з ассета — визначення системи, у якій живуть і крива, і
/// точки Лагранжа. Рахує її `/core`, а не Rust (U6b2).
fn mass_ratio(snapshot: &WorldSnapshot) -> Option<f64> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot.bodies.iter().find(|b| b.body == MOON)?;
    Some(core_rs::cr3bp_mu(earth.mu, moon.mu))
}

/// `C` апарата, за яким малюється крива.
///
/// Першого апарата, а не всіх: крива одна на кадр, і десять напівпрозорих
/// кривих одна поверх одної не сказали б нічого нікому. Апаратів у грі поки
/// один; коли їх стане більше, крива належатиме **обраному** — це вибір
/// інтерфейсу, і робити його наперед тут нема з чого.
fn current_jacobi(snapshot: &WorldSnapshot) -> Option<f64> {
    snapshot.vessels.first()?.jacobi
}

/// Місяць відносно Землі в мить снапшоту.
fn moon_local(snapshot: &WorldSnapshot) -> Option<[f64; 3]> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot.bodies.iter().find(|b| b.body == MOON)?;
    Some([
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ])
}

/// Синодичний базис у мить снапшоту — той, у якому стоять тіла й маркери.
///
/// Тут нормаль береться з **швидкостей** (`d × ḋ`), а не з різниці семплів:
/// снапшот їх має, а сусідньої миті в нього немає. Обидва шляхи дають ту саму
/// площину — це те, що робить кадр цілим.
fn synodic_now(snapshot: &WorldSnapshot) -> Option<Synodic> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot.bodies.iter().find(|b| b.body == MOON)?;

    let d = [
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ];
    let rate = [
        moon.velocity[0] - earth.velocity[0],
        moon.velocity[1] - earth.velocity[1],
        moon.velocity[2] - earth.velocity[2],
    ];
    let normal = [
        d[1] * rate[2] - d[2] * rate[1],
        d[2] * rate[0] - d[0] * rate[2],
        d[0] * rate[1] - d[1] * rate[0],
    ];

    let total = earth.mu + moon.mu;
    if total <= 0.0 {
        return None;
    }
    // Масштаб сталий (`SYNODIC_SCALE_M`), а не теперішня відстань: саме
    // сталість тримає Місяць нерухомим між кадрами.
    Synodic::new(d, normal, frame_view::SYNODIC_SCALE_M, moon.mu / total)
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
            colour: palette::VESSEL.scene(),
        });
    }
}

/// Вмикає рельєф тілу, для якого його завантажили (D12).
///
/// ## Чому окремим викликом, а не параметром `build`
///
/// `TerrainId` видає **кадр** (`Frame::load_terrain`, R5c), а `view::build`
/// про кадр не знає нічого й не має знати: він перетворює снапшот на сцену, і
/// це чиста функція від стану гри. Проносити крізь неї хендл, якого вона не
/// розуміє, означало б зробити двадцять наявних викликів довшими заради того,
/// що стосується двох.
///
/// Тому рельєф — це те, що **додається до готової сцени** тим, хто знає, що
/// саме завантажено. Формулювання чесне й для майбутнього: тіло може мати
/// рельєф на одній машині й не мати на іншій, якщо адаптер не дав bindless
/// (`Frame::load_terrain` там відмовляє), а сцена від цього не стає іншою.
///
/// ## Як тіло знаходиться в сцені
///
/// `engine::scene::Body` не несе ідентифікатора — рушієві він не потрібен, і
/// давати йому знати про `EARTH`/`MOON` означало б навчити рушій грі. Тому
/// індекс рахується **тим самим правилом, яким `build` клав тіла**: порядок
/// `snapshot.bodies`, пропускаючи ті, що без радіуса. Правило одне на дві
/// функції, і саме тому воно тут написане, а не вгадане.
///
/// Мовчить, якщо тіла в сцені немає: снапшот без Місяця — законний стан, а не
/// привід падати.
pub fn attach_terrain(scene: &mut Scene, snapshot: &WorldSnapshot, body: i32, terrain: TerrainId) {
    let mut index = 0;
    for candidate in &snapshot.bodies {
        if candidate.radius_m <= 0.0 {
            continue;
        }
        if candidate.body == body {
            if index < scene.bodies.len() {
                scene.bodies[index].tiles = TileSet::Loaded(terrain);
            }
            return;
        }
        index += 1;
    }
}
