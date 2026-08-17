//! The Earth from 400 km -- what a station crew sees.
//!
//! The same genre as `earth_stills`, and the same trap: it draws with its
//! **own** `Frame`, because `shot::take_scene` creates one of its own in which
//! the surface handle issued here does not exist -- and the scene would
//! silently come out a smooth ball.
//!
//! There is one difference from `earth_stills` and it is the whole point:
//! **the air is on**. From 400 km it is already visible as a band above the
//! limb, and it is at this altitude that what T7h added shows -- a sky lit by
//! the reflection off the surface beneath it.
//!
//! Four angles, each with its own question:
//!
//! * **nadir** -- straight down: is there detail when a grid node (9.8 km) is
//!   wider than a screen pixel;
//! * **limb** -- along the horizon: the band of air against space;
//! * **sunrise** -- the terminator under a low sun: do colour and shadow land
//!   on one surface;
//! * **oblique** -- down at an angle: the limb silhouette together with the
//!   surface in frame.
//!
//!     cargo run --release -p engine --example earth_orbit -- build/orbit

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Atmosphere, Body, Scene, TileSet};
use engine::{frame, shot, tiles};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 900;

/// Orbit altitude, metres -- the same as the ISS and as the `ship_demo` demo.
const ALTITUDE_M: f64 = 400_000.0;

/// The point the station hangs over: the Mediterranean and the Sahara in
/// frame.
const UNDER: (f64, f64) = (28.0, 18.0);

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
    // The body radius is the **asset's reference radius**, not the sphere
    // constant: the heights in the tiles are counted from it, and taking a
    // different number here would raise or sink the whole surface by the
    // difference.
    let radius = terrain.reference_m;
    println!(
        "terrain {} levels, colour {} levels; surface albedo {:?}",
        terrain.levels,
        colour.levels,
        colour.mean().map(|v| (v * 1.0e4).round() / 1.0e4)
    );
    let id = frame.load_surface(&gpu, &terrain, Some(&colour))?;

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("earth orbit"),
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
    let (lat, lon) = (UNDER.0 * degrees, UNDER.1 * degrees);
    // The local basis at the point under the station: up, north, east.
    let up = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];
    let north = [-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos()];
    let east = cross(north, up);

    let eye = scale(up, radius + ALTITUDE_M);

    // `pitch` is the view angle from the nadir: 0 deg straight down, 90 deg
    // along the horizon. `sun` is the Sun's elevation above the local horizon,
    // also in degrees.
    for (name, pitch, sun_elevation, sun_azimuth) in [
        ("nadir", 0.0f64, 55.0f64, 30.0f64),
        ("limb", 88.0, 35.0, 0.0),
        ("sunrise", 84.0, 2.0, 10.0),
        ("oblique", 62.0, 20.0, 60.0),
    ] {
        let (p, e, a) = (
            pitch * degrees,
            sun_elevation * degrees,
            sun_azimuth * degrees,
        );
        // The view: tilted away from the nadir towards the north, i.e.
        // "forward along the motion".
        let forward = unit(add(scale(up, -p.cos()), scale(north, p.sin())));
        // WARNING: the frame's vertical must not be a constant "up from the
        // centre": at the nadir it is parallel to the view, the camera basis
        // degenerates, and the frame comes out black with no diagnostic at
        // all. That is exactly what happened to the first version of this
        // probe. This pair is always orthogonal to the view: at the nadir the
        // frame's vertical becomes north, at the horizon the local vertical.
        let frame_up = unit(add(scale(north, p.cos()), scale(up, p.sin())));
        let target = add(eye, scale(forward, 4.0e6));
        let camera = Camera::look_at(eye, target, frame_up);

        let mut scene = Scene::new(camera);
        // The direction **to** the Sun in the local basis: elevation above the
        // horizon and azimuth from north, clockwise.
        scene.sun = unit(add(
            scale(up, e.sin()),
            add(
                scale(north, e.cos() * a.cos()),
                scale(east, e.cos() * a.sin()),
            ),
        ));
        scene.bodies.push(Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: radius,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Loaded(id),
            colour: frame::COLOUR,
            // The air in its proper place: the top boundary is set relative to
            // **this** radius, not to the constant 6 371 000 inside the
            // constant itself.
            air: Some(Atmosphere::EARTH.with_surface(radius)),
        });

        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("earth orbit"),
            });
        frame.draw(&gpu, &mut commands, &view, WIDTH, HEIGHT, &scene);
        let picture = shot::read_back(&gpu, commands, &texture, WIDTH, HEIGHT)?;
        let path = format!("{dir}/orbit_{name}.png");
        picture.write_png(std::path::Path::new(&path))?;
        println!("  {path}  (pitch {pitch} deg, Sun {sun_elevation} deg above the horizon)");
    }
    Ok(())
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(v: [f64; 3], k: f64) -> [f64; 3] {
    [v[0] * k, v[1] * k, v[2] * k]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}
