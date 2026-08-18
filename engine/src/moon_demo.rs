//! An animated approach to the Moon (stage T, after T3c).
//!
//! A probe, not part of the game -- the same genre as [`crate::ship_demo`] and
//! for the same reason: it shows what has just become possible, and shows it
//! through the **same** [`Frame`] that goes into the window. There is no other
//! render path here.
//!
//! cargo run --release -p engine -- --moon-demo build/moon.apng
//!
//! ## What is shown
//!
//! The camera goes around the Moon along an arc while descending from 1.5e6 to
//! 1.2e5 metres. The two motions together show three things, each added by
//! stage T: surface colour from the LROC WAC mosaic, its agreement with the
//! LOLA terrain, and that **no seam appears** on approach -- neither between
//! tiles nor between pyramid levels, which LOD switches on the way down.
//!
//! The light is fixed while the camera moves -- so the terminator crosses the
//! frame by itself, and it is exactly there that colour and shadow are seen to
//! lie on one surface.
//!
//! The format is APNG, for the same reason as in `ship_demo`: `png` is already
//! a dependency and can do animated PNG, so 60 frames per second are expressed
//! exactly and without a single new dependency.

use std::path::Path;

use crate::frame::Frame;
use crate::gpu::Gpu;
use crate::scene::{Body, Scene, TerrainId, TileSet};
use crate::{demo, shot, tiles};

/// The Moon's radius, metres -- the same as in the other probes.
const RADIUS_M: f64 = 1_737_400.0;

/// Altitude at the start and at the end of the flyby, metres.
///
/// Both bounds are measured rather than eyeballed.
///
/// The lower one comes from frame geometry: the disc fills the frame entirely
/// as soon as the distance drops below `R/sin(30 deg) = 2R`, that is at an
/// altitude of 1.7e6 m. Stopping a little higher keeps the **whole silhouette**
/// in the frame, and with it the terminator, where colour and shadow are seen
/// to lie on one surface.
///
/// WARNING: descending lower makes no sense because of the **data**, not the
/// frame: at 120 km triangle facets become visible. The terrain normal is
/// geometric (R5c), a DEM cell is 5.3 km, so each facet covers tens of pixels
/// and shades flat. That is the limit of the `ldem_4` source, not of the
/// shader.
const FROM_M: f64 = 6.0e6;
const TO_M: f64 = 1.9e6;

/// Where the sub-camera longitude creeps from and to, radians.
///
/// The numbers are not arbitrary: the engine's light stands along `LIGHT_DIR`,
/// that is at 56 deg N and 45 deg E, and the arc is chosen so the camera goes
/// **around it** at a distance of about 40 deg. Then the disc is mostly lit and
/// the terminator runs along the edge -- exactly where colour and shadow are
/// seen to lie on one surface. An arc taken at random gave an almost black
/// ball: the sub-camera point stood sideways to the light.
const LON_FROM: f64 = 0.35;
const LON_TO: f64 = 1.31;

/// Latitude of the sub-camera point, radians.
const LAT: f64 = 0.30;

pub const FRAMES: u32 = 240;
pub const FPS: u16 = 60;

/// The scene for frame number `k` of `frames`.
///
/// Built from scratch every frame on purpose: a scene is data, and a probe that
/// held it between frames would be checking its own cache rather than the
/// frame.
pub fn scene_at(k: u32, frames: u32, tiles: TileSet) -> Scene {
    let t = f64::from(k) / f64::from(frames.max(2) - 1);

    // The altitude falls **geometrically**, not linearly: the eye perceives
    // distance to a surface logarithmically, and a linear descent would look
    // like a stall up top and a lurch at the bottom.
    let altitude = FROM_M * (TO_M / FROM_M).powf(t);
    let distance = RADIUS_M + altitude;

    // Latitude fixed, longitude creeping -- the camera arcs around the
    // light.
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
        // Grey rather than the fixtures' blue: if there is no colour asset,
        // the Moon must stay the Moon rather than a blue ball.
        colour: [0.55, 0.55, 0.56, 1.0],
        air: None,
    });
    scene
}

/// Draws `frames` frames and assembles them into an animated PNG.
///
/// Without surface assets the probe has nothing to show, and staying silent
/// about that is not an option: a smooth grey ball looks like "nothing
/// broke".
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

/// The Moon's terrain and colour from cooked assets.
fn load_surface(gpu: &Gpu, frame: &mut Frame) -> Result<TerrainId, String> {
    // `open` rather than a whole read (X5d): identical for a pyramid no deeper
    // than the resident prefix, which is every asset we cook today, and the
    // streaming path once X5e cooks a deeper one.
    let terrain = tiles::Terrain::open(std::path::Path::new(demo::TERRAIN_ASSET))
        .map_err(|e| format!("{e}\nto fix: make cook-dem"))?;
    let colour = tiles::Colour::open(std::path::Path::new(demo::COLOUR_ASSET))
        .map_err(|e| format!("{e}\nto fix: make cook-colour"))?;

    println!(
        "terrain: {} levels; colour: {} levels",
        terrain.levels, colour.levels
    );
    frame.load_surface(gpu, &terrain, Some(&colour))
}
