//! Вершинна стадія розкладає список трикутників так само, як це робив
//! індексний буфер (ROADMAP-PLANETS.md, R6a).
//!
//! ## Навіщо цей тест існує
//!
//! До R6a зшивання рівнів жило в `cubesphere::indices`: шістнадцять індексних
//! наборів, виклик малювання на патч. Після R6a та сама підміна робиться
//! арифметикою у вершинному шейдері, і кадр малюється одним викликом на тіло.
//!
//! Отже одне правило тепер записане **двічі** — у Rust і в Slang. Це рівно та
//! ситуація, у якій два записи розходяться на четвертій правці, і єдине, що
//! від цього рятує, — сторож, який зіставляє їх напряму.
//!
//! Тут відтворено арифметику шейдера **дослівно**, у тих самих цілих, і
//! звірено з `cubesphere::indices` для всіх шістнадцяти масок. Це не «те саме,
//! написане двічі»: ліва частина — переклад Slang рядок у рядок, права —
//! незалежна реалізація через таблицю вузлів. Збігтися вони можуть лише якщо
//! обидві правильні.
//!
//! Знімок цього не ловить, і це виміряно: `--shot` після R6a бітово той самий,
//! що до нього, — але сцена зондів рушія має п'ять патчів, і **жодного зшитого
//! ребра**. Тобто бітова рівність кадру доводить розкладку трикутників і
//! нічого не каже про підміну вузлів.

use engine::cubesphere::{self, SIDE};

/// Переклад `node_of` зі `shaders/patch.slang` рядок у рядок.
///
/// Свідомо незграбний: `u32`, ділення з остачею, ті самі імена. Якщо колись
/// захочеться написати це «гарніше» — саме тоді він і перестане бути звіркою
/// з шейдером.
fn node_of(vertex: u32, mask: u32) -> u32 {
    const SIDE_U: u32 = SIDE as u32;
    const NODES: u32 = SIDE_U + 1;

    let triangle = vertex / 3;
    let corner = vertex % 3;
    let cell = triangle / 2;
    let half = triangle % 2;

    let mut a = cell / SIDE_U;
    let mut b = cell % SIDE_U;

    let first = [(0u32, 0u32), (1, 0), (0, 1)];
    let second = [(0u32, 1u32), (1, 0), (1, 1)];
    let step = if half == 0 {
        first[corner as usize]
    } else {
        second[corner as usize]
    };
    a += step.0;
    b += step.1;

    let odd_on_b = a % 2 == 1 && ((b == 0 && mask & 4 != 0) || (b == SIDE_U && mask & 8 != 0));
    let odd_on_a = b % 2 == 1 && ((a == 0 && mask & 1 != 0) || (a == SIDE_U && mask & 2 != 0));
    if odd_on_b {
        a -= 1;
    }
    if odd_on_a {
        b -= 1;
    }

    a * NODES + b
}

/// Для всіх шістнадцяти масок арифметика шейдера дає той самий список вузлів,
/// що й індексний буфер — вершина за вершиною, у тому самому порядку.
#[test]
fn the_shader_walks_the_same_triangles_as_the_index_buffer() {
    let count = SIDE * SIDE * 6;
    for mask in 0..16u8 {
        let expected = cubesphere::indices(mask);
        assert_eq!(expected.len(), count);

        for (vertex, &wanted) in expected.iter().enumerate() {
            let by_shader = node_of(vertex as u32, u32::from(mask));
            assert_eq!(
                by_shader, wanted,
                "маска {mask:04b}, вершина {vertex}: шейдер дає вузол \
                 {by_shader}, індексний буфер — {wanted}"
            );
        }
    }
    println!("  {count} вершин × 16 масок збіглися до одного вузла");
}

/// Сітка в шейдері й сітка в коді — те саме число.
///
/// `SIDE` записаний і в `cubesphere`, і в `shaders/patch.slang` як
/// `static const uint SIDE = 32`. Спільної константи між Rust і Slang не
/// існує, тож лишається сторож — і саме тому він дивиться в **файл шейдера**,
/// а не повторює число.
#[test]
fn the_shader_and_the_code_agree_on_the_patch_size() {
    let source = include_str!("../shaders/patch.slang");
    let wanted = format!("static const uint SIDE = {SIDE};");
    assert!(
        source.contains(&wanted),
        "у shaders/patch.slang немає рядка «{wanted}» — сітка розійшлася з \
         cubesphere::SIDE, і кадр малюватиме інші трикутники"
    );
}

// ---------------------------------------------------------------------------
// Відбір у compute (R6b)

use engine::camera::Camera;
use engine::cull;
use engine::frame::{Frame, FOV_Y};
use engine::gpu::Gpu;
use engine::lod;
use engine::scene::{Body, Scene, TileSet};
use engine::shot;

const SIZE: u32 = 256;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// **Оракул, заради якого R3 робився на CPU.**
///
/// Кількість патчів, яку намалював GPU, мусить збігтися з тією, яку відібрав
/// CPU, — на тих самих вісьмох камерах, що в R2c. Два незалежні шляхи, одне
/// число. Без цього помилка GPU-відбору виглядає як «десь щось не
/// намалювалось» і шукається очима.
///
/// Збіг вимагається **точний**, і це не самовпевненість: обидва шляхи рахують
/// ту саму формулу, а різниця арифметики (`f64` на CPU проти `f32` на GPU)
/// може зіграти лише на патчі, який стоїть рівно на межі відбору. План кроку
/// назвав цю розвилку наперед: якщо збіг не досягається, звужувати треба
/// твердження, а не допуск. Виміряно — звужувати не довелося.
#[test]
fn the_gpu_draws_exactly_as_many_patches_as_the_cpu_kept() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cull shot"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
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
    let mut frame = Frame::new(&gpu, shot::FORMAT);

    let focal = lod::focal_px(FOV_Y, f64::from(SIZE));
    let aspect = 1.0;
    let mut checked = 0;

    for &x in &[-1.0f64, 1.0] {
        for &y in &[-1.0f64, 1.0] {
            for &z in &[-1.0f64, 1.0] {
                for altitude in [1.0e5, 3.0e5, 4.0e6] {
                    let length = (x * x + y * y + z * z).sqrt();
                    let distance = EARTH_RADIUS_M + altitude;
                    let eye = [
                        x / length * distance,
                        y / length * distance,
                        z / length * distance,
                    ];
                    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);

                    // Шлях CPU: той самий вибір рівня, той самий відбір.
                    let body = lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M);
                    let selection = lod::select(&body, &camera, focal, None);
                    let occluder =
                        cull::Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M, body.rotation);
                    let mut visibility = cull::horizon(&selection, &occluder, &camera);
                    cull::frustum(
                        &mut visibility,
                        &selection,
                        &occluder,
                        &camera,
                        FOV_Y,
                        aspect,
                    );

                    // Шлях GPU: намалювати кадр і спитати лічильник indirect.
                    let mut scene =
                        Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
                    scene.bodies.push(Body {
                        centre: [0.0, 0.0, 0.0],
                        radius_m: EARTH_RADIUS_M,
                        orientation: [1.0, 0.0, 0.0, 0.0],
                        tiles: TileSet::Smooth,
                    });
                    let mut encoder =
                        gpu.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("cull"),
                            });
                    frame.draw(&gpu, &mut encoder, &view, SIZE, SIZE, &scene);
                    gpu.queue.submit([encoder.finish()]);

                    let drawn = frame
                        .drawn_patches(&gpu)
                        .expect("лічильник мав прочитатися");

                    assert_eq!(
                        drawn[0] as usize,
                        visibility.drawn(),
                        "напрямок ({x}, {y}, {z}), висота {altitude:.1e} м: GPU \
                         намалював {} патчів, CPU лишив {} з {}",
                        drawn[0],
                        visibility.drawn(),
                        selection.patches.len()
                    );
                    checked += 1;
                }
            }
        }
    }

    println!("  {checked} камер: GPU і CPU відібрали порівну на кожній");
}
