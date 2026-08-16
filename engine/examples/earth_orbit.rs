//! Земля з висоти 400 км — те, що бачить екіпаж станції.
//!
//! Той самий жанр, що `earth_stills`, і та сама пастка: малює **своїм**
//! `Frame`, бо `shot::take_scene` створює власний, у якому виданого тут
//! хендла поверхні не існує — і сцена тихо вийшла б гладкою кулею.
//!
//! Відмінність від `earth_stills` одна й уся тут: **повітря увімкнене**. З
//! 400 км його вже видно смугою над лімбом, і саме на цій висоті видно те, що
//! T7h додав, — небо, підсвічене відбиттям від поверхні під ним.
//!
//! Чотири ракурси, кожен зі своїм питанням:
//!
//! * **nadir** — прямо вниз: чи є деталь, коли вузол сітки (9.8 км) ширший за
//!   екранний піксель;
//! * **limb** — уздовж горизонту: смуга повітря на тлі космосу;
//! * **sunrise** — термінатор під косим сонцем: чи лягають колір і тінь на
//!   одну поверхню;
//! * **oblique** — навскіс униз: силует лімба разом з поверхнею в кадрі.
//!
//!     cargo run --release -p engine --example earth_orbit -- build/orbit

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Atmosphere, Body, Scene, TileSet};
use engine::{frame, shot, tiles};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 900;

/// Висота орбіти, метри — та сама, що в МКС і в демо `ship_demo`.
const ALTITUDE_M: f64 = 400_000.0;

/// Під якою точкою висить станція: Середземне море й Сахара в кадрі.
const UNDER: (f64, f64) = (28.0, 18.0);

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
    // Радіус тіла — **опорний радіус ассета**, а не стала сфери: висоти в
    // тайлах відлічені саме від нього, і брати сюди інше число означало б
    // підняти або втопити всю поверхню на різницю.
    let radius = terrain.reference_m;
    println!(
        "рельєф {} рівнів, колір {} рівнів; альбедо поверхні {:?}",
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
    // Місцевий базис у точці під станцією: вгору, на північ, на схід.
    let up = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];
    let north = [-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos()];
    let east = cross(north, up);

    let eye = scale(up, radius + ALTITUDE_M);

    // `pitch` — кут погляду від надира: 0° прямо вниз, 90° уздовж горизонту.
    // `sun` — висота Сонця над місцевим горизонтом, теж у градусах.
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
        // Погляд: від надира відхиляємось на північ, тобто «вперед по руху».
        let forward = unit(add(scale(up, -p.cos()), scale(north, p.sin())));
        // ⚠ Вертикаль кадру не можна брати сталою «вгору від центра»: у надирі
        // вона паралельна погляду, і базис камери вироджується — кадр виходить
        // чорним, без жодної діагностики. Це рівно те, що сталося з першою
        // версією зонда. Ця пара завжди ортогональна до погляду: у надирі
        // вертикаллю кадру стає північ, на горизонті — місцева вертикаль.
        let frame_up = unit(add(scale(north, p.cos()), scale(up, p.sin())));
        let target = add(eye, scale(forward, 4.0e6));
        let camera = Camera::look_at(eye, target, frame_up);

        let mut scene = Scene::new(camera);
        // Напрямок **до** Сонця в місцевому базисі: висота над горизонтом і
        // азимут від півночі за годинниковою.
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
            // Повітря на своєму місці: верхня межа задана відносно **цього**
            // радіуса, а не сталої 6 371 000 в самій константі.
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
        println!("  {path}  (нахил {pitch}°, Сонце {sun_elevation}° над горизонтом)");
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
