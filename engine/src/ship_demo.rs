//! A demo animation of the ship in frame (stage V, step V2).
//!
//! This is a probe, not a part of the game: it shows what V2 has just made
//! possible -- a hull metres from the camera and a planet millions of metres
//! away in the same frame. It is drawn by the **very same** [`Frame`] that
//! goes to the window; there is no other render path here and there must not
//! be one (ROADMAP, "Рендер": the frame knows nothing about the window).
//!
//! ## Why APNG, and not a series of PNGs or a video
//!
//! We do not write codecs ourselves (CLAUDE.md, "Чого НЕ робимо"), while
//! `png` is already among the dependencies and can do animated PNG -- so 60
//! frames per second is expressed exactly, `set_frame_delay(1, 60)`, with no
//! new dependency and no external `ffmpeg`, which may not be on the machine. A
//! series of files would only play back for someone who can assemble them.
//!
//! ## What exactly is shown
//!
//! The ship on a circular 400 km orbit, nose along the motion; the camera goes
//! around it in a circle over the animation, while the ship itself makes one
//! turn about its nose. The two motions together show what the asymmetric
//! shape is for: the nose, the plane of the fins, and the roll -- by the
//! porthole.

use std::path::Path;

use crate::chase::{self, Chase};
use crate::frame::Frame;
use crate::gpu::Gpu;
use crate::mesh::Model;
use crate::scene::{Atmosphere, Body, Scene, Ship, TileSet};
use crate::{ship, shot, sphere};

/// The cooked hull. Without it the V1 stub is drawn.
pub const SHIP_ASSET: &str = "assets/ship.mesh";

/// Altitude of the demo orbit, metres.
const ALTITUDE_M: f64 = 400_000.0;

/// How many metres from the camera to the ship.
const RANGE_M: f64 = 14.0;

pub const FRAMES: u32 = 240;
pub const FPS: u16 = 60;

/// Bounding-sphere radius of the V1 stub -- in fractions of the height.
///
/// The number comes from `ship::generate`: the fins stick out to 1.9 of the
/// largest radius, i.e. further than the nose. It lives here rather than in
/// `ship` because it is the scene that needs it -- as a fallback for when
/// there is no model on disk.
pub const STUB_EXTENT: f64 = 0.5;

/// The scene for frame number `k` out of `frames`.
///
/// Built from scratch every frame on purpose: a scene is data, and a probe
/// that held one between frames would be checking its cache rather than the
/// frame.
pub fn scene_at(k: u32, frames: u32, extent: f64) -> Scene {
    let phase = 2.0 * std::f64::consts::PI * f64::from(k) / f64::from(frames);

    // The local basis on the orbit: `up` points away from the planet's centre.
    let radius = sphere::EARTH_RADIUS_M + ALTITUDE_M;
    let centre = [radius, 0.0, 0.0];
    let up = [1.0, 0.0, 0.0];

    // Nose along the motion: a quarter turn about `+X` takes the ship's `+Z`
    // to the world's `+Y`. Plus one turn about the nose over the whole
    // animation -- that is what shows the porthole and makes the roll visible.
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
        // The bounding-sphere radius is a property of the **shape**, so it
        // comes from the model when there is one (T5d3). Both `near` and the
        // third-person camera distance rest on it, so a constant here would
        // mean a camera that does not notice the hull has changed.
        extent_m: extent * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: crate::ship::HULL_ROUGHNESS,
        metallic: crate::ship::HULL_METALLIC,
    };

    // The camera is the same third person as in the game (V4), and it circles
    // the ship by the very same mouse drag the player steers it with. A full
    // turn per animation is `2*pi`/`RADIANS_PER_PIXEL` pixels; a probe setting
    // the angles past `drag` would be showing a path the game does not have.
    //
    // A quarter turn back from the typical angle -- so that the animation
    // opens from the side rather than from the nose: the ship's nose lies
    // along the world's `+Y`, i.e. exactly where `Chase` looks by default, and
    // from the nose the silhouette reads worst of all.
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

/// Product of quaternions `[w, x, y, z]`: `b` first, then `a`.
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

/// Read the cooked hull if it is there (stage T, step T5d3).
///
/// A missing asset is not an error: `/assets/` is not in git, so a clean clone
/// will not have it until `make model-ship && make cook-ship` has run. The
/// probe then draws the procedural V1 stub -- exactly as a body without a
/// tileset is drawn in its own colour.
pub fn hull() -> Option<Model> {
    let bytes = match std::fs::read(SHIP_ASSET) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("no hull at {SHIP_ASSET} ({e}) -- drawing the stub.");
            eprintln!("to fix: make cook-ship");
            return None;
        }
    };
    match Model::from_bytes(&bytes) {
        Ok(model) => {
            println!(
                "hull: {SHIP_ASSET}, {} vertices, model length {:.2} m",
                model.mesh.positions.len(),
                model.height_m
            );
            Some(model)
        }
        Err(e) => {
            eprintln!("the hull {SHIP_ASSET} does not read ({e}) -- drawing the stub.");
            None
        }
    }
}

/// Draws `frames` frames and assembles them into an animated PNG.
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

    // One [`Frame`] for the whole animation: the air tables are computed once,
    // the patch cache outlives a frame, and the probe measures the same thing
    // the window does.
    let mut frame = Frame::new(gpu, shot::FORMAT);

    // The hull from the asset if it has been cooked. The ship's height is left
    // to the scene -- in the file the mesh is unit-sized, and how many metres
    // this vessel is, the game decides, not the model. `extent`, on the other
    // hand, is a property of the shape, and the scene takes it from there.
    let hull = hull();
    if let Some(model) = &hull {
        frame.load_ship(gpu, model);
    }

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
    // Exactly 1/60 of a second per frame -- not "about 60 fps" but an exact
    // fraction.
    encoder
        .set_frame_delay(1, FPS)
        .map_err(|e| format!("APNG: {e}"))?;
    let mut writer = encoder.write_header().map_err(|e| format!("APNG: {e}"))?;

    let extent = hull.as_ref().map_or(STUB_EXTENT, |model| model.extent);

    for k in 0..frames {
        let scene = scene_at(k, frames, extent);

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
