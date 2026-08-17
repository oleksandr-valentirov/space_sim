//! The ship from four sides as separate PNGs -- to look at with the eye (T9).
//!
//! The same genre as `earth_stills`, and the same trap: it draws with its
//! **own** `Frame`, because `shot::take_scene` creates one of its own in which
//! the mesh loaded here does not exist -- and the V1 stub would go into frame.
//!
//! Why not `--ship-demo`: that makes a 240-frame APNG, i.e. it answers the
//! question "how does the ship rotate". Shape and paint are asked about from
//! the side, from the nose and from the stern, and they are asked while
//! standing still.
//!
//!     cargo run --release -p engine --example ship_stills -- build/ship

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::mesh::Model;
use engine::scene::{Scene, Ship};
use engine::{frame, ship, shot};

const WIDTH: u32 = 720;
const HEIGHT: u32 = 900;

/// How many metres from the camera to the ship.
const RANGE_M: f64 = 13.0;

fn main() -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());

    let mut frame = frame::Frame::new(&gpu, shot::FORMAT);
    let model = Model::from_bytes(
        &std::fs::read("assets/ship.mesh")
            .map_err(|e| format!("assets/ship.mesh: {e}\nto fix: make cook-ship"))?,
    )?;
    println!(
        "mesh: {} vertices, {} triangles, height {:.3} m, extent {:.3}",
        model.mesh.positions.len(),
        model.mesh.indices.len() / 3,
        model.height_m,
        model.extent
    );
    frame.load_ship(&gpu, &model);

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ship stills"),
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

    let height_m = ship::DEFAULT_HEIGHT_M;
    let ship = Ship {
        centre: [0.0, 0.0, 0.0],
        // The identity rotation: the nose along the world's `+Z`, i.e. up in
        // frame.
        orientation: [1.0, 0.0, 0.0, 0.0],
        height_m,
        extent_m: model.extent * height_m,
        colour: [1.0, 1.0, 1.0, 1.0],
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    };

    for (name, azimuth) in [
        ("porthole", 0.0f64),
        ("fin", 45.0),
        ("side", 90.0),
        ("back", 180.0),
    ] {
        let angle = azimuth * std::f64::consts::PI / 180.0;
        // The camera in the plane of the porthole (the ship's `+X`) and a
        // little above: from there both the silhouette and the fact that the
        // rim sits on a convex hull are visible.
        let eye = [RANGE_M * angle.cos(), RANGE_M * angle.sin(), 0.25 * RANGE_M];
        let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
        // The light source to the side and above the camera: direct light
        // from behind would make the frame flat, and light head-on would leave
        // nothing of the ship but a silhouette.
        scene.sun = unit([
            angle.cos() * 0.6 - angle.sin(),
            angle.sin() * 0.6 + angle.cos(),
            0.55,
        ]);
        scene.ships.push(ship);

        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ship stills"),
            });
        frame.draw(&gpu, &mut commands, &view, WIDTH, HEIGHT, &scene);
        let picture = shot::read_back(&gpu, commands, &texture, WIDTH, HEIGHT)?;
        let path = format!("{dir}/ship_{name}.png");
        picture.write_png(std::path::Path::new(&path))?;
        println!("  {path}");
    }
    Ok(())
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}
