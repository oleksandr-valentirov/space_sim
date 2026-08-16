//! Демо-анімація корабля в кадрі (етап V, крок V2).
//!
//! Це зонд, а не частина гри: він показує те, що V2 щойно зробив можливим —
//! корпус за метри від камери й планета за мільйони метрів у тому самому
//! кадрі. Малює його **той самий** [`Frame`], що йде у вікно; іншого шляху
//! рендера тут немає й не має бути (ROADMAP «Рендер»: кадр не знає про вікно).
//!
//! ## Чому APNG, а не серія PNG і не відео
//!
//! Кодек не пишемо самі (PROJECT.md, «чого НЕ робимо»), а `png` у залежностях
//! уже є й уміє анімований PNG — тобто 60 кадрів на секунду виражаються
//! точно, `set_frame_delay(1, 60)`, без жодної нової залежності й без
//! зовнішнього `ffmpeg`, якого на машині може не бути. Серія файлів
//! програвалася б лише в тому, хто вміє їх зібрати.
//!
//! ## Що саме показано
//!
//! Корабель на круговій орбіті 400 км, ніс уздовж руху; камера обходить його
//! колом за час анімації, а сам він робить один оберт навколо носа. Обидва
//! рухи разом показують те, заради чого форма несиметрична: ніс, площину
//! стабілізаторів і крен — по ілюмінатору.

use std::path::Path;

use crate::chase::{self, Chase};
use crate::frame::Frame;
use crate::gpu::Gpu;
use crate::scene::{Atmosphere, Body, Scene, Ship, TileSet};
use crate::{ship, shot, sphere};

/// Висота орбіти демонстрації, метри.
const ALTITUDE_M: f64 = 400_000.0;

/// Скільки метрів від камери до корабля.
const RANGE_M: f64 = 14.0;

pub const FRAMES: u32 = 240;
pub const FPS: u16 = 60;

/// Сцена на кадр номер `k` з `frames`.
///
/// Побудована з нуля щокадру навмисно: сцена — це дані, і зонд, який тримав
/// би її між кадрами, перевіряв би свій кеш, а не кадр.
pub fn scene_at(k: u32, frames: u32) -> Scene {
    let phase = 2.0 * std::f64::consts::PI * f64::from(k) / f64::from(frames);

    // Місцевий базис на орбіті: `up` — від центра планети.
    let radius = sphere::EARTH_RADIUS_M + ALTITUDE_M;
    let centre = [radius, 0.0, 0.0];
    let up = [1.0, 0.0, 0.0];

    // Ніс уздовж руху: чверть оберту навколо `+X` переводить корабельний `+Z`
    // у світовий `+Y`. Плюс один оберт навколо носа за всю анімацію — саме
    // він показує ілюмінатор і робить крен видимим.
    let nose = [
        (-std::f64::consts::FRAC_PI_4).cos(),
        (-std::f64::consts::FRAC_PI_4).sin(),
        0.0,
        0.0,
    ];
    let roll = [(phase / 2.0).cos(), 0.0, (phase / 2.0).sin(), 0.0];

    let ship = Ship {
        centre,
        orientation: multiply(roll, nose),
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: crate::ship::HULL_ROUGHNESS,
        metallic: crate::ship::HULL_METALLIC,
    };

    // Камера — та сама третя особа, що в грі (V4), і обходить корабель вона
    // тим самим тягненням миші, яким її водить гравець. Повний оберт за
    // анімацію — 2π/`RADIANS_PER_PIXEL` пікселів; зонд, який ставив би кути
    // повз `drag`, показував би шлях, якого в грі немає.
    //
    // Чверть оберту назад від типового кута — щоб анімація відкривалася
    // збоку, а не з носа: ніс корабля лежить уздовж світового `+Y`, тобто
    // рівно там, куди дивиться `Chase` за замовчуванням, а з носа силует
    // читається гірше за все.
    let mut chase = Chase::at_ranges(RANGE_M / ship.extent_m);
    chase.drag(
        (phase - std::f64::consts::FRAC_PI_2) / chase::RADIANS_PER_PIXEL,
        0.0,
    );
    let camera = chase.camera(&ship, up);

    let mut scene = Scene::new(camera);
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: crate::frame::COLOUR,
        air: Some(Atmosphere::EARTH),
    });
    scene.ships.push(ship);

    scene
}

/// Добуток кватерніонів `[w, x, y, z]`: спершу `b`, потім `a`.
fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

/// Малює `frames` кадрів і складає їх в анімований PNG.
pub fn render(gpu: &Gpu, width: u32, height: u32, frames: u32, path: &Path) -> Result<(), String> {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ship demo"),
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

    // Один [`Frame`] на всю анімацію: таблиці повітря рахуються раз, кеш
    // патчів переживає кадр, і зонд міряє те саме, що робить вікно.
    let mut frame = Frame::new(gpu, shot::FORMAT);

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
    // Рівно 1/60 секунди на кадр — не «приблизно 60 fps», а точний дріб.
    encoder
        .set_frame_delay(1, FPS)
        .map_err(|e| format!("APNG: {e}"))?;
    let mut writer = encoder.write_header().map_err(|e| format!("APNG: {e}"))?;

    for k in 0..frames {
        let scene = scene_at(k, frames);

        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ship demo"),
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
