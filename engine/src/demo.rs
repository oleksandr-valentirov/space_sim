//! Демонстрація поточного стану рендера: серія знімків без вікна.
//!
//! Не новий шлях і не окремий рендер. Це той самий [`crate::frame::Frame`],
//! той самий [`crate::shot`] і ті самі сцени, якими користуються тести —
//! просто зібрані в один прогін і підписані. Тому демка **не може**
//! показати те, чого немає в грі: якщо картинка вийшла, значить рушій справді
//! це малює.
//!
//! ## Чому знімками, а не вікном
//!
//! З тієї самої причини, з якої існує `--shot` (ROADMAP F1): вікно нічого не
//! доводить, а знімок можна покласти в комміт, надіслати й порівняти. Демка
//! перевідтворюється однією командою й дає ті самі байти:
//!
//! ```sh
//! make cook-dem                                  # раз, якщо assets/ порожній
//! cargo run --release -p engine -- --demo build/demo
//! ```
//!
//! Каталог належить тому, хто його назвав: демка пише в нього свої файли й
//! **нічого не видаляє**. Перейменований кадр лишить по собі старий файл, і
//! прибрати його — справа того, хто перейменовував.
//!
//! ## Чого тут свідомо немає
//!
//! **Жодного власного шейдера, кольору чи камери «щоб гарніше».** Освітлення
//! тимчасове й таким і виглядає; колір планети — той самий `COLOUR` з
//! `frame.rs`. Демка, підфарбована окремо, показувала б себе, а не рушій.

use std::path::Path;

use crate::camera::Camera;
use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::scene::{Body, Polyline, Scene, TerrainId, TileSet};
use crate::shot;
use crate::tiles::Terrain;
use crate::{live, sphere};

/// Розмір кадру демки.
///
/// Не 1280×720: знімки читаються поруч один з одним, а не поодинці, і
/// вчетверо менший файл важить тут більше за вчетверо більший піксель.
const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

const MOON_RADIUS_M: f64 = 1_737_400.0;

/// Скукований рельєф Місяця, від кореня репозиторію.
pub const TERRAIN_ASSET: &str = "assets/moon.dem";

/// Один кадр демки: ім'я файлу й підпис, що саме на ньому видно.
pub struct Picture {
    pub name: &'static str,
    pub caption: String,
}

/// Камера на висоті `altitude` над напрямком `direction`, дивиться в центр.
fn above(direction: [f64; 3], radius_m: f64, altitude: f64, up: [f64; 3]) -> Camera {
    let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
    let distance = radius_m + altitude;
    let eye = direction.map(|v| v / length * distance);
    Camera::look_at(eye, [0.0, 0.0, 0.0], up)
}

/// Одиничний напрямок на джерело світла — той самий, що освітлює кадр.
fn light() -> [f64; 3] {
    let l = frame::LIGHT_DIR.map(f64::from);
    let n = l.iter().map(|v| v * v).sum::<f64>().sqrt();
    l.map(|v| v / n)
}

/// Напрямок під кутом `tilt` градусів до світла.
///
/// Потрібен рівно для того, щоб не знімати нічний бік: там освітлення стале,
/// і рельєф на ньому невидимий ні з тайлами, ні без них. Перша версія тестів
/// R5c саме на це й наступила.
fn from_light(tilt: f64) -> [f64; 3] {
    let l = light();
    let seed = if l[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let e1 = unit(cross(l, seed));
    let (c, s) = (tilt.to_radians().cos(), tilt.to_radians().sin());
    [0, 1, 2].map(|k| c * l[k] + s * e1[k])
}

/// Камера на висоті `altitude`, що дивиться **вздовж лімба**, а не вниз.
///
/// Погляд у надир з низької орбіти дає рівне поле кольору й не показує
/// нічого: сфера накриває кадр цілком, а гладка сфера ще й не має чого
/// показувати. Перша версія демки саме такою й вийшла, і це було чесно, але
/// марно. Лімб натомість показує все одразу — кривизну, ближню площину,
/// відбір за горизонтом і профіль рельєфу проти неба.
///
/// Ціль погляду — точка поверхні рівно на горизонті: `acos(R / (R + h))` від
/// підкамерної. Її рахує арифметика, а не око.
fn along_limb(direction: [f64; 3], radius_m: f64, altitude: f64) -> Camera {
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let u = unit(direction);
    // Дотична до сфери в підкамерній точці: будь-яка, аби перпендикулярна.
    let seed = if u[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let side = unit([
        u[1] * seed[2] - u[2] * seed[1],
        u[2] * seed[0] - u[0] * seed[2],
        u[0] * seed[1] - u[1] * seed[0],
    ]);
    let tangent = [
        side[1] * u[2] - side[2] * u[1],
        side[2] * u[0] - side[0] * u[2],
        side[0] * u[1] - side[1] * u[0],
    ];

    let distance = radius_m + altitude;
    let eye = u.map(|v| v * distance);
    let horizon = (radius_m / distance).acos();
    let (c, s) = (horizon.cos(), horizon.sin());
    let target = [0, 1, 2].map(|k| radius_m * (c * u[k] + s * tangent[k]));
    // Вертикаль кадру — назовні від тіла: так небо вгорі, а поверхня внизу.
    Camera::look_at(eye, target, u)
}

fn body(radius_m: f64, tiles: TileSet) -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        air: None,
    }
}

/// Сцена з halo-орбіти: Земля, Місяць і траєкторія, порахована **зараз**.
///
/// Єдина сцена демки, у якій є фізика. Лінія — вихід `prop_run` крізь поле
/// десяти тіл ассета (H5), а не колонка з CSV.
///
/// ## Чому в обертовому фреймі, а не у світових координатах
///
/// Перша версія цієї сцени малювала світові координати й дала пряму лінію
/// через увесь кадр — і це не помилка рендера, а правда про масштаб: за
/// дванадцять діб система Земля-Місяць пролітає геліоцентрично на порядки
/// більше, ніж розмах самої halo-орбіти. Та сама причина, з якої
/// `trajectory_render` бере **геоцентричний** anchor (F6).
///
/// Тут фрейм обертовий (`trajectory::rotating_position`), а масштаб
/// **закріплений** сталою відстанню, а не миттєвою: інакше Місяць дихав би
/// разом з ексцентриситетом своєї орбіти (U6a3).
fn halo() -> Result<Scene, String> {
    // Середня відстань Земля-Місяць. Обертовий фрейм безрозмірний, і саме
    // цією сталою він повертається в метри.
    const L: f64 = 3.844e8;

    let asset = live::repo_asset();
    let flight =
        live::propagate(&live::fixture_start(), 14.0, &asset).map_err(|e| format!("{e:?}"))?;
    let samples = &flight.samples;
    if samples.len() < 2 {
        return Err("прогноз повернув менше двох семплів".to_string());
    }

    let points: Vec<[f64; 3]> = samples
        .iter()
        .map(|s| {
            let p = crate::trajectory::rotating_position(s.vessel, s.earth, s.moon, s.z_axis);
            [p[0] * L, p[1] * L, p[2] * L]
        })
        .collect();

    let moon = [(1.0 - crate::trajectory::MU) * L, 0.0, 0.0];
    let earth = [-crate::trajectory::MU * L, 0.0, 0.0];

    // Кадр будується з даних: центр — середина хмари точок разом із Місяцем,
    // відстань — з її розмаху. Підбирати це руками означало б, що знімок
    // перестане бути правильним, щойно орбіта зміниться.
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in points.iter().chain(std::iter::once(&moon)) {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    let centre = [0, 1, 2].map(|k| (low[k] + high[k]) / 2.0);
    let extent = (0..3)
        .map(|k| high[k] - low[k])
        .fold(0.0_f64, f64::max)
        .max(1.0);

    // Погляд збоку й трохи згори: halo-орбіта не пласка, і фронтальний
    // погляд показав би її як відрізок.
    let eye = [
        centre[0] - extent * 0.35,
        centre[1] - extent * 1.25,
        centre[2] + extent * 0.55,
    ];

    let mut scene = Scene::new(Camera::look_at(eye, centre, [0.0, 0.0, 1.0]));
    scene.bodies.push(Body {
        centre: earth,
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        air: None,
    });
    scene.bodies.push(Body {
        centre: moon,
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        air: None,
    });
    scene.polylines.push(Polyline {
        points,
        colour: [1.0, 0.75, 0.25, 1.0],
    });
    Ok(scene)
}

/// Намалювати всю серію в каталог `out`.
pub fn render(gpu: &Gpu, out: &Path) -> Result<Vec<Picture>, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);

    // Рельєф — з готового ассета. Його відсутність не мовчазна: сцени з
    // тайлами зникають, і про це сказано вголос, з командою, яка це лікує.
    let terrain: Option<TerrainId> = match std::fs::read(TERRAIN_ASSET) {
        Ok(bytes) => {
            let data = Terrain::from_bytes(&bytes)?;
            let levels = data.levels;
            let id = frame.load_terrain(gpu, &data)?;
            println!("рельєф: {TERRAIN_ASSET}, {levels} рівнів піраміди");
            Some(id)
        }
        Err(e) => {
            println!("рельєфу немає ({TERRAIN_ASSET}: {e}) — сцени з тайлами пропущено.");
            println!("полікувати: make cook-dem");
            None
        }
    };

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("demo"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;

    let mut taken = Vec::new();
    let mut shoot = |name: &'static str, caption: String, scene: &Scene| -> Result<(), String> {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("demo"),
            });
        frame.draw(gpu, &mut encoder, &view, WIDTH, HEIGHT, scene);
        let picture = shot::read_back(gpu, encoder, &texture, WIDTH, HEIGHT)?;
        let path = out.join(format!("{name}.png"));
        picture.write_png(&path)?;
        println!("  {}", path.display());
        taken.push(Picture { name, caption });
        Ok(())
    };

    // 1. Земля здалеку — той самий кадр, що дає `--shot`.
    let mut scene = Scene::new(above(
        from_light(35.0),
        sphere::EARTH_RADIUS_M,
        frame::DEFAULT_ALTITUDE_M,
        [0.0, 0.0, 1.0],
    ));
    scene
        .bodies
        .push(body(sphere::EARTH_RADIUS_M, TileSet::Smooth));
    shoot(
        "01_earth_far",
        "Земля з 10⁷ м. LOD віддає планеті шість граней куба — на цій \
         відстані дрібніший поділ не зсунув би жодного пікселя."
            .to_string(),
        &scene,
    )?;

    // 2. Земля з низької орбіти, погляд уздовж лімба.
    let mut scene = Scene::new(along_limb(from_light(35.0), sphere::EARTH_RADIUS_M, 3.0e5));
    scene
        .bodies
        .push(body(sphere::EARTH_RADIUS_M, TileSet::Smooth));
    shoot(
        "02_earth_low",
        "Земля з 300 км, погляд уздовж лімба. Той самий критерій дав дев'ять \
         патчів замість шести, і горизонт прибрав більше половини з них ще до \
         малювання. Ближня площина міряється від найближчого тіла сцени — \
         інакше на цій висоті вона зрізала б поверхню під ногами."
            .to_string(),
        &scene,
    )?;

    if let Some(id) = terrain {
        // 3 і 4 — пара з однієї камери: без тайлів і з ними.
        for (name, tiles, what) in [
            ("03_moon_smooth", TileSet::Smooth, "без тайлів"),
            ("04_moon_terrain", TileSet::Loaded(id), "з тайлами LOLA"),
        ] {
            let mut scene = Scene::new(above(
                from_light(30.0),
                MOON_RADIUS_M,
                1.2e6,
                [0.0, 0.0, 1.0],
            ));
            scene.bodies.push(body(MOON_RADIUS_M, tiles));
            shoot(
                name,
                format!(
                    "Місяць з 1.2·10⁶ м, {what}. Пара знімків з однієї камери: \
                     різницю дає рівно висота, зсунута вздовж нормалі патча."
                ),
                &scene,
            )?;
        }

        // 5. Термінатор — те, заради чого R5c робився.
        let mut scene = Scene::new(above(from_light(72.0), MOON_RADIUS_M, 1.2e6, light()));
        scene.bodies.push(body(MOON_RADIUS_M, TileSet::Loaded(id)));
        shoot(
            "05_moon_terminator",
            "Місяць на термінаторі. Сонце падає навскіс, і нахил кожної \
             фасетки вирішує, освітлена вона чи ні: повна варіація яскравості \
             тут у 9.7 раза більша, ніж у гладкої сфери."
                .to_string(),
            &scene,
        )?;

        // 6 і 7 — друга пара, зблизька й уздовж лімба. Пара, а не один
        // знімок, з тієї самої причини, що й вище: «рельєф видно» без
        // другої картинки поруч — це твердження, яке нікому не перевірити.
        for (name, tiles, what) in [
            ("06_moon_limb_smooth", TileSet::Smooth, "без тайлів"),
            ("07_moon_limb_terrain", TileSet::Loaded(id), "з тайлами"),
        ] {
            let mut scene = Scene::new(along_limb(from_light(35.0), MOON_RADIUS_M, 1.0e5));
            scene.bodies.push(body(MOON_RADIUS_M, tiles));
            shoot(
                name,
                format!(
                    "Місяць зі 100 км уздовж лімба, {what}. Горизонт за 570 км, \
                     і рельєф там міняє сам силует. Фасетки видно, і це чесно: \
                     LDEM_4 дає 7581 м на відлік, тобто 47 екранних пікселів на \
                     клітинку — деталь нижче за дані це вже крок R7."
                ),
                &scene,
            )?;
        }
    }

    // 7. Фізика в кадрі: halo-орбіта, порахована зараз.
    match halo() {
        Ok(scene) => shoot(
            "08_halo",
            "Halo-орбіта, порахована `prop_run` крізь поле десяти тіл ассета — \
             не прочитана з CSV. Фрейм обертовий, масштаб закріплений сталою \
             відстанню; Місяць праворуч, Земля за кадром ліворуч. Кадр \
             будується з даних, а не підібраний руками."
                .to_string(),
            &scene,
        )?,
        Err(e) => println!("сцену halo пропущено: {e}"),
    }

    Ok(taken)
}
