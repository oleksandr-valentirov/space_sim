//! Анімація підльоту до Місяця (етап T, після T3c).
//!
//! Зонд, а не частина гри — той самий жанр, що [`crate::ship_demo`], і з тієї
//! самої причини: він показує те, що щойно стало можливим, і показує це
//! **тим самим** [`Frame`], який іде у вікно. Іншого шляху рендера тут немає.
//!
//! cargo run --release -p engine -- --moon-demo build/moon.apng
//!
//! ## Що саме показано
//!
//! Камера обходить Місяць по дузі й водночас знижується з 1.5·10⁶ до 1.2·10⁵
//! метрів. Два рухи разом показують три речі, кожну з яких додав етап T:
//! колір поверхні з мозаїки LROC WAC, його узгодження з рельєфом LOLA, і те,
//! що на підльоті **не з'являється шва** — ні між тайлами, ні між рівнями
//! піраміди, які LOD міняє дорогою вниз.
//!
//! Світило нерухоме, а камера рухається — тож термінатор проходить по кадру
//! сам собою, і саме на ньому видно, що колір і тіні лежать на одній поверхні.
//!
//! Формат — APNG, з тієї ж причини, що в `ship_demo`: `png` у залежностях уже
//! є й уміє анімований PNG, тобто 60 кадрів на секунду виражаються точно й без
//! жодної нової залежності.

use std::path::Path;

use crate::frame::Frame;
use crate::gpu::Gpu;
use crate::scene::{Body, Scene, TerrainId, TileSet};
use crate::{demo, shot, tiles};

/// Радіус Місяця, метри — той самий, що в решті зондів.
const RADIUS_M: f64 = 1_737_400.0;

/// Висота на початку й у кінці прольоту, метри.
///
/// Обидві межі виміряні, а не підібрані на око.
///
/// Нижня — з геометрії кадру: диск заповнює кадр цілком, щойно відстань падає
/// нижче `R/sin(30°) = 2R`, тобто на висоті 1.7·10⁶ м. Зупинка трохи вище
/// лишає в кадрі **весь силует**, а з ним і термінатор, на якому й видно, що
/// колір і тіні лежать на одній поверхні.
///
/// ⚠ Нижче спускатися нема сенсу не через кадр, а через **дані**: на 120 км у
/// кадрі стають видимі грані трикутників. Нормаль рельєфу геометрична (R5c),
/// клітинка DEM — 5.3 км, отже кожна фасетка накриває десятки пікселів і
/// світиться рівно. Це межа джерела `ldem_4`, а не шейдера.
const FROM_M: f64 = 6.0e6;
const TO_M: f64 = 1.9e6;

/// Звідки й куди повзе довгота підкамерної точки, радіани.
///
/// Числа не з голови: світило рушія стоїть у напрямку `LIGHT_DIR`, тобто на
/// 56° пн. і 45° сх., і дуга обрана так, щоб камера йшла **навколо нього** на
/// відстані близько 40°. Тоді диск здебільшого освітлений, а термінатор
/// проходить по краю — саме там видно, що колір і тіні лежать на одній
/// поверхні. Дуга, взята навмання, дала майже чорну кулю: підкамерна точка
/// стояла до світила боком.
const LON_FROM: f64 = 0.35;
const LON_TO: f64 = 1.31;

/// Широта підкамерної точки, радіани.
const LAT: f64 = 0.30;

pub const FRAMES: u32 = 240;
pub const FPS: u16 = 60;

/// Сцена на кадр номер `k` з `frames`.
///
/// Будується з нуля щокадру навмисно: сцена — це дані, і зонд, який тримав би
/// її між кадрами, перевіряв би свій кеш, а не кадр.
pub fn scene_at(k: u32, frames: u32, tiles: TileSet) -> Scene {
    let t = f64::from(k) / f64::from(frames.max(2) - 1);

    // Висота падає **геометрично**, а не лінійно: очима відстань до поверхні
    // сприймається логарифмічно, і лінійне зниження виглядало б як зупинка
    // вгорі й ривок унизу.
    let altitude = FROM_M * (TO_M / FROM_M).powf(t);
    let distance = RADIUS_M + altitude;

    // Широта фіксована, довгота повзе — камера обходить світило по дузі.
    let lon = LON_FROM + t * (LON_TO - LON_FROM);
    let lat = LAT;
    let eye = [
        distance * lat.cos() * lon.cos(),
        distance * lat.cos() * lon.sin(),
        distance * lat.sin(),
    ];

    let camera = crate::camera::Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = Scene::new(camera);
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        // Сірий, а не синій колір фікстур: якщо колірного асета не буде,
        // Місяць має лишитись Місяцем, а не блакитною кулею.
        colour: [0.55, 0.55, 0.56, 1.0],
        air: None,
    });
    scene
}

/// Малює `frames` кадрів і складає їх в анімований PNG.
///
/// Без асетів поверхні зонд не має чого показувати, і мовчати про це не можна:
/// гладка сіра куля виглядає як «нічого не зламалось».
pub fn render(gpu: &Gpu, width: u32, height: u32, frames: u32, path: &Path) -> Result<(), String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let surface = load_surface(gpu, &mut frame)?;

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("moon demo"),
        size: wgpu::Extent3d {
            width,
            height,
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

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .set_animated(frames, 0)
        .map_err(|e| format!("APNG: {e}"))?;
    encoder
        .set_frame_delay(1, FPS)
        .map_err(|e| format!("APNG: {e}"))?;
    let mut writer = encoder.write_header().map_err(|e| format!("APNG: {e}"))?;

    for k in 0..frames {
        let scene = scene_at(k, frames, TileSet::Loaded(surface));
        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("moon demo"),
            });
        frame.draw(gpu, &mut commands, &view, width, height, &scene);
        let shot = shot::read_back(gpu, commands, &texture, width, height)?;
        writer
            .write_image_data(&shot.pixels)
            .map_err(|e| format!("APNG: {e}"))?;
    }

    writer.finish().map_err(|e| format!("APNG: {e}"))?;
    Ok(())
}

/// Рельєф і колір Місяця з готових асетів.
fn load_surface(gpu: &Gpu, frame: &mut Frame) -> Result<TerrainId, String> {
    let bytes = std::fs::read(demo::TERRAIN_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-dem", demo::TERRAIN_ASSET))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;

    let bytes = std::fs::read(demo::COLOUR_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-colour", demo::COLOUR_ASSET))?;
    let colour = tiles::Colour::from_bytes(&bytes)?;

    println!(
        "рельєф: {} рівнів; колір: {} рівнів",
        terrain.levels, colour.levels
    );
    frame.load_surface(gpu, &terrain, Some(&colour))
}
