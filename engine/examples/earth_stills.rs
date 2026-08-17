//! The Earth from three altitudes as separate PNGs -- to look at with the eye
//! (T7g).
//!
//! The same genre as `moon_stills`, and the same trap: it draws with its
//! **own** `Frame`, because `shot::take_scene` creates one of its own in which
//! the surface handle issued here does not exist -- and the scene would
//! silently come out a smooth ball.
//!
//! The altitudes are chosen so that each answers its own question:
//!
//! * **1e7 m** -- the whole disc in frame: are the continents where they
//!   belong or not;
//! * **1e6 m** -- a continent filling the frame: is the coastline sharp;
//! * **2e5 m** -- low orbit: is there detail when a grid node (9.8 km) is
//!   wider than a screen pixel.
//!
//! The camera looks at the point beneath it, the light source is off to the
//! side of the view direction so that the terminator enters the frame: colour
//! and shadows have to lie on one surface.
//!
//!     cargo run --release -p engine --example earth_stills -- build/earth

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::{frame, shot, sphere, tiles};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

/// Where the camera looks -- Europe and Africa in frame, i.e. the most
/// recognisable half of the globe. Latitude and longitude in degrees.
const LOOK_AT: (f64, f64) = (20.0, 15.0);

fn main() -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());

    let mut frame = frame::Frame::new(&gpu, shot::FORMAT);
    let terrain = tiles::Terrain::from_bytes(
        &std::fs::read("assets/earth.dem")
            .map_err(|e| format!("assets/earth.dem: {e}\nto fix: make cook-earth"))?,
    )?;
    let colour = tiles::Colour::from_bytes(
        &std::fs::read("assets/earth.col")
            .map_err(|e| format!("assets/earth.col: {e}\nto fix: make cook-earth"))?,
    )?;
    println!(
        "terrain: {} levels; colour: {} levels, {} channels, sRGB {}",
        terrain.levels, colour.levels, colour.channels, colour.srgb
    );
    let id = frame.load_surface(&gpu, &terrain, Some(&colour))?;
    // The same surface without the mosaic -- so one can ask where an artefact
    // came from: the colour or the terrain under it.
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
        // The light source off to the side: the terminator crosses the frame,
        // and on it one can see that colour and shadow lie on one surface
        // rather than in two layers.
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
