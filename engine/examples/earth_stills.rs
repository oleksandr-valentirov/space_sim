//! Земля з трьох висот як окремі PNG — щоб подивитись оком (T7g).
//!
//! Той самий жанр, що `moon_stills`, і та сама пастка: малює **своїм**
//! `Frame`, бо `shot::take_scene` створює власний, у якому виданого тут
//! хендла поверхні не існує — і сцена тихо вийшла б гладкою кулею.
//!
//! Висоти взяті так, щоб кожна відповідала на своє питання:
//!
//! * **10⁷ м** — увесь диск у кадрі: континенти на своїх місцях чи ні;
//! * **10⁶ м** — материк на весь кадр: чи видно берегову лінію різкою;
//! * **2·10⁵ м** — низька орбіта: чи є деталь, коли вузол сітки (9.8 км)
//!   ширший за екранний піксель.
//!
//! Камера дивиться на точку під собою, світило — збоку від напрямку погляду,
//! щоб термінатор входив у кадр: колір і тіні мусять лежати на одній поверхні.
//!
//!     cargo run --release -p engine --example earth_stills -- build/earth

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::{frame, shot, sphere, tiles};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

/// Куди дивиться камера — Європа й Африка в кадрі, тобто найупізнаваніша
/// половина глобуса. Широта й довгота в градусах.
const LOOK_AT: (f64, f64) = (20.0, 15.0);

fn main() -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}", gpu.describe());

    let mut frame = frame::Frame::new(&gpu, shot::FORMAT);
    let terrain = tiles::Terrain::from_bytes(
        &std::fs::read("assets/earth.dem")
            .map_err(|e| format!("assets/earth.dem: {e}\nполікувати: make cook-earth"))?,
    )?;
    let colour = tiles::Colour::from_bytes(
        &std::fs::read("assets/earth.col")
            .map_err(|e| format!("assets/earth.col: {e}\nполікувати: make cook-earth"))?,
    )?;
    println!(
        "рельєф: {} рівнів; колір: {} рівнів, {} канали, sRGB {}",
        terrain.levels, colour.levels, colour.channels, colour.srgb
    );
    let id = frame.load_surface(&gpu, &terrain, Some(&colour))?;
    // Та сама поверхня без мозаїки — щоб питати, звідки взявся артефакт:
    // з кольору чи з рельєфу під ним.
    let grey = frame.load_surface(&gpu, &terrain, None)?;

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("earth stills"),
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

    let dir = std::env::args().nth(1).unwrap_or_else(|| "build".into());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let degrees = std::f64::consts::PI / 180.0;
    let (lat, lon) = (LOOK_AT.0 * degrees, LOOK_AT.1 * degrees);
    let under = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];

    for (name, tiles, altitude) in [
        ("disc", TileSet::Loaded(id), 1.0e7f64),
        ("close", TileSet::Loaded(id), 1.0e6),
        ("low", TileSet::Loaded(id), 2.0e5),
        ("grey", TileSet::Loaded(grey), 1.0e6),
        ("smooth", TileSet::Smooth, 1.0e6),
    ] {
        let range = sphere::EARTH_RADIUS_M + altitude;
        let eye = [under[0] * range, under[1] * range, under[2] * range];
        let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
        // Світило збоку: термінатор проходить кадром, і на ньому видно, що
        // колір і тінь лежать на одній поверхні, а не двома шарами.
        scene.sun = unit([
            under[0] * 0.5 - under[1],
            under[1] * 0.5 + under[0],
            under[2] * 0.5 + 0.3,
        ]);
        scene.bodies.push(Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: sphere::EARTH_RADIUS_M,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles,
            colour: frame::COLOUR,
            air: None,
        });

        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("earth stills"),
            });
        frame.draw(&gpu, &mut commands, &view, WIDTH, HEIGHT, &scene);
        let picture = shot::read_back(&gpu, commands, &texture, WIDTH, HEIGHT)?;
        let path = format!("{dir}/earth_{name}.png");
        picture.write_png(std::path::Path::new(&path))?;
        println!("  {path}");
    }
    Ok(())
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}
