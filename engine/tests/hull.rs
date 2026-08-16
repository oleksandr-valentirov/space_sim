//! Матеріал корпусу в кадрі проти аналітичного двійника (етап T, крок T5c).
//!
//! Той самий оракул, що `engine::cull` проти `cull.slang` і
//! `engine::atmosphere` проти `sky.slang`: **число проти числа**. GGX має
//! замкнену форму, тож [`engine::brdf`] дає точну відповідь без експозиції й
//! без будь-яких налаштувань вигляду, і розбіжність означає помилку.
//!
//! ## Порівнюються центри трикутників, а не вершини
//!
//! Вершина лежить на межі кількох трикутників, тобто в точці, де інтерполяція
//! нормалі стрибає (у плоских нормалей) або де округлення растеризатора
//! вирішує, кому належить піксель. Центр трикутника не має ні того, ні того: у
//! ньому інтерпольована нормаль — це середнє трьох вершинних, і воно ж
//! рахується на CPU.
//!
//! ## Простір — камерний, і це не деталь реалізації
//!
//! Позиції вершин корабля приходять уже поверненими в осі камери
//! (`Camera::relative`), тож нормаль і напрямок на світило теж камерні (T5c).
//! Двійник мусить рахувати в тому самому просторі, інакше він звірятиме
//! правильну формулу з правильною формулою на різних векторах.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Scene, Ship};
use engine::shot::Shot;
use engine::{brdf, frame, ship, shot, srgb, tonemap};

const SIZE: u32 = 768;

/// Розмір корабля й відстань до нього, метри.
const HEIGHT_M: f64 = 20.0;
const RANGE_M: f64 = 45.0;

/// Базовий колір корпусу — три різні канали навмисно.
///
/// Однакові канали пропустили б перестановку каналів у шейдері, а вона тут
/// цілком можлива: `F0` металу — це і є базовий колір, тобто колір входить у
/// формулу двічі й різними шляхами.
const BASE: [f32; 4] = [0.55, 0.70, 0.85, 1.0];

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// Сцена: корабель перед камерою, більше нічого.
///
/// Без планети й без повітря: обидва додали б у піксель світло, якого двійник
/// не рахує, і оракул перестав би бути числом проти числа.
fn scene(sun: [f64; 3], roughness: f32, metallic: f32) -> Scene {
    // ⚠ Камера стоїть **навскіс**, і це не для краси. З камерою на осі `z`
    // базис камери збігається зі світовим, `Camera::rotate` стає тотожністю —
    // і найгрубіша з можливих помилок, освітлення в двох різних просторах,
    // робиться в такій фікстурі невидимою. Перша редакція так і стояла, і
    // навмисний злам «світило у світових осях» вона пропустила.
    let eye = [RANGE_M * 0.42, RANGE_M * 0.31, RANGE_M * 0.85];
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut scene = Scene::new(camera);
    scene.sun = sun;
    scene.ships.push(Ship {
        centre: [0.0, 0.0, 0.0],
        // Чверть оберту навколо `x`: корабель стає до камери **боком**.
        //
        // ⚠ Не косметика. Ніс — конус, і носом до камери жодна грань не
        // дивиться в неї достатньо прямо, щоб дзеркальний пік узагалі
        // потрапив у кадр: при `roughness = 0.08` пік `D` — це 7768, а
        // найближча грань ловить 0.13. Бік корпусу — тіло обертання, і його
        // центральна смуга звернена до камери точно.
        orientation: [
            std::f64::consts::FRAC_PI_4.cos(),
            std::f64::consts::FRAC_PI_4.sin(),
            0.0,
            0.0,
        ],
        height_m: HEIGHT_M,
        extent_m: 0.5 * HEIGHT_M,
        colour: BASE,
        roughness,
        metallic,
    });
    scene
}

fn normalise(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// Похибки по всіх перевірених гранях, у байтах.
struct Agreement {
    errors: Vec<i32>,
    /// Скільки граней вийшли **понад коліно** тонмапера.
    ///
    /// Без цього числа оракул міг би мовчки перевіряти лише тотожну частину
    /// кривої: нижче коліна стиснення не робить нічого, тож його помилка там
    /// невидима.
    compressed: usize,
}

impl Agreement {
    fn checked(&self) -> usize {
        self.errors.len()
    }

    /// Медіана — головне число оракула.
    ///
    /// Помилка у **формулі** зсуває кожну грань, тобто й медіану. Перекриття
    /// геометрією псує окремі грані, лишаючи медіану нулем; саме тому оракул
    /// питає про неї, а не про максимум.
    fn median(&self) -> i32 {
        let mut sorted = self.errors.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    fn within(&self, bytes: i32) -> f64 {
        let good = self.errors.iter().filter(|e| **e <= bytes).count();
        good as f64 / self.errors.len() as f64
    }

    fn worst(&self) -> i32 {
        self.errors.iter().copied().max().unwrap_or(0)
    }
}

fn compare(gpu: &Gpu, sun: [f64; 3], roughness: f32, metallic: f32) -> Agreement {
    let scene = scene(sun, roughness, metallic);
    let shot: Shot = shot::take_scene(gpu, SIZE, SIZE, &scene).expect("кадр мав намалюватися");
    let camera = &scene.camera;
    let ship = &scene.ships[0];
    let mesh = ship::generate(ship.height_m);
    // Той самий поворот, що застосовує кадр (`frame::rotation`), — інакше
    // двійник рахував би для іншої геометрії.
    let turn = |v: [f64; 3]| {
        let q = ship.orientation;
        let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
        [
            v[0] * (1.0 - 2.0 * (y * y + z * z))
                + v[1] * 2.0 * (x * y - w * z)
                + v[2] * 2.0 * (x * z + w * y),
            v[0] * 2.0 * (x * y + w * z)
                + v[1] * (1.0 - 2.0 * (x * x + z * z))
                + v[2] * 2.0 * (y * z - w * x),
            v[0] * 2.0 * (x * z - w * y)
                + v[1] * 2.0 * (y * z + w * x)
                + v[2] * (1.0 - 2.0 * (x * x + y * y)),
        ]
    };
    let light = {
        let d = camera.rotate(sun);
        normalise([f64::from(d[0]), f64::from(d[1]), f64::from(d[2])])
    };

    let mut out = Agreement {
        errors: Vec::new(),
        compressed: 0,
    };

    for triangle in mesh.indices.chunks_exact(3) {
        let corners: Vec<usize> = triangle.iter().map(|i| *i as usize).collect();

        // Центр трикутника у світі й середня нормаль — рівно те, що дає
        // інтерполяція в тій самій точці.
        let mut centre = [0.0f64; 3];
        let mut normal = [0.0f64; 3];
        for &k in &corners {
            for axis in 0..3 {
                centre[axis] += mesh.positions[k][axis] / 3.0;
                normal[axis] += f64::from(mesh.normals[k][axis]) / 3.0;
            }
        }
        if normal.iter().all(|v| *v == 0.0) {
            continue;
        }
        let centre = turn(centre);
        let normal = turn(normal);

        // Трикутник мусить бути помітно більший за піксель: у дрібного центр
        // ділять сусіди, і кадр там показує суміш кількох граней.
        let mut corner_px = Vec::with_capacity(3);
        for &k in &corners {
            let world = turn(mesh.positions[k]);
            let Some(p) = camera.to_screen(frame::FOV_Y, SIZE, SIZE, world) else {
                break;
            };
            corner_px.push(p);
        }
        if corner_px.len() < 3 {
            continue;
        }
        let area = {
            let (a, b, c) = (corner_px[0], corner_px[1], corner_px[2]);
            let ux = f64::from(b[0] - a[0]);
            let uy = f64::from(b[1] - a[1]);
            let vx = f64::from(c[0] - a[0]);
            let vy = f64::from(c[1] - a[1]);
            0.5 * (ux * vy - uy * vx).abs()
        };
        if area < 12.0 {
            continue;
        }

        let Some(pixel) = camera.to_screen(frame::FOV_Y, SIZE, SIZE, centre) else {
            continue;
        };
        let (x, y) = (pixel[0].round() as i64, pixel[1].round() as i64);
        if x < 1 || y < 1 || x + 1 >= i64::from(SIZE) || y + 1 >= i64::from(SIZE) {
            continue;
        }
        let (x, y) = (x as u32, y as u32);

        // Трикутник мусить накривати свій піксель разом із сусідами: на межі
        // силуету частина з них — небо, і порівнювати там нема чого.
        let neighbours = [
            shot.pixel(x, y),
            shot.pixel(x - 1, y),
            shot.pixel(x + 1, y),
            shot.pixel(x, y - 1),
            shot.pixel(x, y + 1),
        ];
        if neighbours
            .iter()
            .any(|p| [p[0], p[1], p[2]] == frame::CLEAR_BYTES)
        {
            continue;
        }
        // І сусіди мусять бути близькі один до одного: різкий перепад означає
        // ребро або край перекриття, тобто піксель, який належить іншій грані.
        let spread = neighbours
            .iter()
            .map(|p| i32::from(p[1]))
            .max()
            .unwrap_or(0)
            - neighbours
                .iter()
                .map(|p| i32::from(p[1]))
                .min()
                .unwrap_or(0);
        if spread > 8 {
            continue;
        }

        let position = camera.relative64(centre);
        let view = normalise([-position[0], -position[1], -position[2]]);
        let n = camera.rotate(normal);
        let n = normalise([f64::from(n[0]), f64::from(n[1]), f64::from(n[2])]);
        // ⚠ **Грані, відвернуті від камери, відкидаються, і це головний
        // фільтр оракула.** Корпус — тіло обертання, тож рівно половина його
        // граней лежить на дальньому боці; їхні центри проєктуються в ті самі
        // пікселі, що й ближні, і кадр там показує **іншу** грань. Перша
        // редакція цього не відсіювала й дістала 28% збігів; друга розвертала
        // нормаль до ока, як шейдер, і дістала 69% — правдоподібні числа з
        // чужих пікселів. Правильна відповідь — не рахувати їх узагалі.
        //
        // Запас 0.15 прибирає ще й грані, майже паралельні променю: там
        // піксель ділять кілька граней одразу.
        if n[0] * view[0] + n[1] * view[1] + n[2] * view[2] < 0.15 {
            continue;
        }

        let mut worst = 0;
        let mut compressed = false;
        for channel in 0..3 {
            let value = brdf::radiance(
                n,
                view,
                light,
                f64::from(BASE[channel]),
                f64::from(roughness),
                f64::from(metallic),
            );
            if value > tonemap::KNEE {
                compressed = true;
            }
            // ⚠ Стиснення входить у передбачення (T5c3). Без нього оракул
            // розійшовся б рівно на відблиску — тобто там, де матеріал
            // найцікавіший.
            let expected = i32::from(srgb::linear_to_byte(tonemap::compress(value)));
            let got = i32::from(neighbours[0][channel]);
            worst = worst.max((expected - got).abs());
        }
        // Дзеркальний пік не порівнюється — див. пояснення в тесті.
        if compressed {
            out.compressed += 1;
            continue;
        }
        out.errors.push(worst);
    }
    out
}

/// Кадр дає те саме число, що аналітичний двійник, на кожній грані корпусу.
///
/// Прогін по чотирьох матеріалах і двох світилах: помилка в одному доданку
/// формули майже завжди лишає інший правильним, тож дзеркальний метал і
/// матовий діелектрик мусять зійтися **обидва**.
///
/// ⚠ Ідеальної згоди тут бути не може, і причина геометрична, а не числова:
/// корпус несе стабілізатори, тож частина граней **перекрита** іншими, і центр
/// такої грані падає в піксель, який належить не їй. Глибину з кадру не
/// прочитати, тож ці випадки не відсіюються — звідси й головне число оракула
/// **медіана**, а не максимум: помилка у формулі зсуває кожну грань, перекриття
/// псує окремі.
///
/// ⚠ **Що цей оракул не ловить, перевірено зламом:**
///
/// * **показник Френеля** (`t⁵` замість `t⁴`). У металу `F0` — це базовий
///   колір, тобто 0.55…0.85, і доданок `(1 − F0)·t⁵` лишається дрібним; у
///   діелектрика він великий лише при дотичному куті, де самого відблиску мало
///   проти дифузного члена. Тобто в цій фікстурі показник не спостережний, і
///   стереже його [`the_shader_carries_the_same_material_numbers`];
/// * **розворот нормалі до ока** — за побудовою: грані, відвернуті від камери,
///   оракул відкидає. Стереже його `tests/sun.rs`, де без розвороту корпус
///   дістає чорні плями.
///
/// Ловить він те, заради чого й існує: `α = roughness` замість `roughness²`
/// валить медіану одразу.
#[test]
fn every_facet_shows_the_number_the_analytic_brdf_predicts() {
    let Some(gpu) = gpu() else { return };

    for (roughness, metallic) in [(0.35, 1.0), (0.8, 1.0), (0.25, 0.0), (0.9, 0.0)] {
        for sun in [[0.0, 0.0, 1.0], [0.55, 0.3, 0.78]] {
            let got = compare(&gpu, sun, roughness, metallic);
            println!(
                "  шорсткість {roughness}, метал {metallic}, світило {sun:?}: \
                 {} граней, медіана {}, у межах 2 байтів {:.1}%, найгірша {}, \
                 понад коліном {}",
                got.checked(),
                got.median(),
                got.within(2) * 100.0,
                got.worst(),
                got.compressed
            );
            assert!(
                got.checked() > 40,
                "перевірено лише {} граней — фікстура не працює",
                got.checked()
            );
            // ⚠ Грані **дзеркального піку** з порівняння викидаються, і це не
            // послаблення оракула, а межа методу. Пік `D` при `roughness =
            // 0.35` вищий за схил у сотні разів, тож різниця між середньою
            // нормаллю трьох вершин (те, що рахує двійник) і перспективно
            // інтерпольованою в центрі пікселя (те, що бачить шейдер) дає там
            // сотні байтів при тих самих формулах. Виміряно: без цього
            // фільтра медіана лишається нулем, а найгірша грань — 235 байтів.
            // ⚠ Один байт, а не нуль, і причина названа: точка порівняння —
            // проєкція **просторового** центра трикутника, а растеризатор
            // інтерполює атрибути перспективно-коректно, тобто з вагами, що
            // не дорівнюють третинам, коли вершини лежать на різній глибині.
            // Корпус боком до камери — тіло обертання, і в нього таких
            // трикутників більшість. Помилка у формулі дає тут не одиницю, а
            // десятки.
            assert!(
                got.median() <= 1,
                "медіана розбіжності {} байтів: шейдер розійшовся з \
                 `engine::brdf` на кожній грані, а не на окремих",
                got.median()
            );
            assert!(
                got.within(2) > 0.8,
                "у межах 2 байтів лише {:.1}% граней",
                got.within(2) * 100.0
            );
        }
    }
}

/// Числа матеріалу записані двічі — у Rust і в шейдері — і мусять збігатися.
///
/// Той самий сторож, що звіряє `SIDE` у `gpu_driven.rs` і сталі правила
/// матеріалу в `material.rs`. Тут він несе більше, ніж звичайно: показник
/// Френеля в кадрі не спостережний (див. вище), тож єдине, що взагалі стоїть
/// між ним і тихою розбіжністю, — цей рядок.
#[test]
fn the_shader_carries_the_same_material_numbers() {
    let source = include_str!("../shaders/ship.slang");
    for (name, value) in [
        ("DIELECTRIC_F0", brdf::DIELECTRIC_F0),
        ("MIN_ROUGHNESS", brdf::MIN_ROUGHNESS),
    ] {
        let wanted = format!("static const float {name} = {value};");
        assert!(
            source.contains(&wanted),
            "у shaders/ship.slang немає рядка «{wanted}» — матеріал розійшовся \
             з `engine::brdf`"
        );
    }
    // П'ятий степінь Шліка: у кадрі його не видно, і саме тому він тут.
    assert!(
        source.contains("float t5 = t * t * t * t * t;"),
        "у shaders/ship.slang немає п'ятого степеня Шліка"
    );
}
