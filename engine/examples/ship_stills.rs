//! Корабель з чотирьох боків як окремі PNG — щоб подивитись оком (T9).
//!
//! Той самий жанр, що `earth_stills`, і та сама пастка: малює **своїм**
//! `Frame`, бо `shot::take_scene` створює власний, у якому завантаженого тут
//! меша не існує — і в кадр поїхала б заглушка V1.
//!
//! Чому не `--ship-demo`: він робить APNG на 240 кадрів, тобто відповідає на
//! питання «як корабель обертається». Форму й фарбу питають з боку, з носа й
//! з корми, і питають нерухомо.
//!
//!     cargo run --release -p engine --example ship_stills -- build/ship

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::mesh::Model;
use engine::scene::{Scene, Ship};
use engine::{frame, ship, shot};

const WIDTH: u32 = 720;
const HEIGHT: u32 = 900;

/// Скільки метрів від камери до корабля.
const RANGE_M: f64 = 13.0;

fn main() -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}", gpu.describe());

    let mut frame = frame::Frame::new(&gpu, shot::FORMAT);
    let model = Model::from_bytes(
        &std::fs::read("assets/ship.mesh")
            .map_err(|e| format!("assets/ship.mesh: {e}\nполікувати: make cook-ship"))?,
    )?;
    println!(
        "меш: {} вершин, {} трикутників, висота {:.3} м, extent {:.3}",
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
        // Тотожний поворот: ніс уздовж світового `+Z`, тобто вгору кадром.
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
        // Камера в площині ілюмінатора (корабельний `+X`) і трохи згори:
        // звідти видно і силует, і те, що обідок стоїть на опуклому корпусі.
        let eye = [RANGE_M * angle.cos(), RANGE_M * angle.sin(), 0.25 * RANGE_M];
        let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
        // Світило збоку-згори від камери: пряме світло в спину зробило б кадр
        // пласким, а зустрічне лишило б від корабля силует.
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
