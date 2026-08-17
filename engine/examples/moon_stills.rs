//! Three frames of the approach animation as separate PNGs -- to look at
//! with the eye.
//!
//! WARNING: it draws with its **own** `Frame` rather than through
//! `shot::take_scene`: that one creates a frame of its own inside, so the
//! surface handle issued here does not exist in it, and the scene is silently
//! drawn smooth. The first version of this example did exactly that, and the
//! Moon came out an even blue ball.
//!
//! cargo run --release -p engine --example moon_stills -- build/stills

use engine::gpu::Gpu;
use engine::scene::TileSet;
use engine::{demo, frame, moon_demo, shot, tiles};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

fn main() -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());

    let mut frame = frame::Frame::new(&gpu, shot::FORMAT);
    let bytes = std::fs::read(demo::TERRAIN_ASSET).map_err(|e| e.to_string())?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;
    let bytes = std::fs::read(demo::COLOUR_ASSET).map_err(|e| e.to_string())?;
    let colour = tiles::Colour::from_bytes(&bytes)?;
    let id = frame.load_surface(&gpu, &terrain, Some(&colour))?;

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("moon stills"),
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

    for k in [0u32, 120, 239] {
        let scene = moon_demo::scene_at(k, moon_demo::FRAMES, TileSet::Loaded(id));
        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("moon stills"),
            });
        frame.draw(&gpu, &mut commands, &view, WIDTH, HEIGHT, &scene);
        let picture = shot::read_back(&gpu, commands, &texture, WIDTH, HEIGHT)?;
        let path = format!("{dir}/still_{k:03}.png");
        picture.write_png(std::path::Path::new(&path))?;
        println!("  {path}");
    }
    Ok(())
}
