//! Кадр малює скукований меш, а не заглушку (етап T, крок T5d3).
//!
//! Оракул той самий, що у V2, і саме тому він тут щось означає: силует у
//! кадрі проти проєкції **вершин самої моделі** через `Camera::to_screen` —
//! дві незалежні реалізації одного перетворення. Різниця лише в тому, звідки
//! беруться вершини: з асета, а не з `ship::generate`.
//!
//! ## Модель у тесті синтетична, і це навмисно
//!
//! Справжній асет лежить у `assets/`, якого немає в git (`.gitignore`), тож
//! тест, що читає його з диска, у CI перевіряв би відсутність файлу. Тут
//! модель будується кодом — і будується **несиметричною за всіма трьома
//! осями**, бо симетрична ховає і переставлені осі, і поворот, і масштаб
//! (D13, D14 прожили саме так).

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::mesh::Model;
use engine::scene::{Scene, Ship};
use engine::shot::{self, Shot};
use engine::sphere::Mesh;
use engine::{frame, ship};

const SIZE: u32 = 256;
const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const DISTANCE: f64 = 15.0;

/// Довжина моделі в метрах — не та, з якою корабель стоїть у сцені.
///
/// Числа різні навмисно: кукер нормалізує меш до одиничної висоти, а сцена
/// множить його на свою. Якби вони збігалися, забутий поділ виглядав би
/// правильно.
const MODEL_M: f64 = 3.0;
const SHIP_M: f64 = 8.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// Клин: довгий уздовж `+Z`, ширший ліворуч, з одним зрізаним кутом.
///
/// Замкнена оболонка з шести вершин; жодна площина симетрії її в себе не
/// переводить, тож і поворот, і перестановка осей у кадрі видимі.
fn wedge(length_m: f64) -> Mesh {
    let h = 0.5 * length_m;
    let w = 0.28 * length_m;
    let positions = vec![
        [-w, -0.6 * w, -h],
        [1.7 * w, -0.4 * w, -h],
        [0.2 * w, 1.3 * w, -h],
        [-0.5 * w, -0.2 * w, h],
        [0.9 * w, -0.5 * w, h],
        [0.1 * w, 0.4 * w, h],
    ];
    // Нормалі тут не є оракулом (силует їх не питає), але брехати ними теж
    // не можна: беремо напрямок від центра, як у V1.
    let normals = positions
        .iter()
        .map(|p: &[f64; 3]| {
            let n = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            [(p[0] / n) as f32, (p[1] / n) as f32, (p[2] / n) as f32]
        })
        .collect();
    let indices = vec![
        0, 2, 1, // хвіст
        3, 4, 5, // ніс
        0, 1, 4, 0, 4, 3, // борт
        1, 2, 5, 1, 5, 4, // борт
        2, 0, 3, 2, 3, 5, // борт
    ];
    Mesh {
        positions,
        normals,
        indices,
    }
}

fn model() -> Model {
    Model::from_metres(wedge(MODEL_M)).expect("клин — це модель")
}

fn scene_with(orientation: [f64; 4], extent: f64) -> Scene {
    let eye = [DISTANCE, 0.0, 0.0];
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = Scene::new(camera);
    scene.ships.push(Ship {
        centre: [0.0, 0.0, 0.0],
        orientation,
        height_m: SHIP_M,
        extent_m: extent * SHIP_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    });
    scene
}

/// Кадр із мешем на вибір: `None` — заглушка V1, `Some` — скукована модель.
fn take(gpu: &Gpu, scene: &Scene, model: Option<&Model>) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ship asset shot"),
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
    let mut frame = frame::Frame::new(gpu, shot::FORMAT);
    if let Some(model) = model {
        frame.load_ship(gpu, model);
    }
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ship asset"),
        });
    frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, scene);
    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав вийти")
}

/// Прямокутник, у який вписані всі непорожні пікселі.
fn lit_bounds(shot: &Shot) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bounds
}

/// Той самий силует на CPU: вершини моделі, помножені на висоту корабля.
fn projected_bounds(camera: &Camera, model: &Model) -> (f64, f64, f64, f64) {
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &model.mesh.positions {
        let world = [p[0] * SHIP_M, p[1] * SHIP_M, p[2] * SHIP_M];
        let screen = camera
            .to_screen(FOV_Y, SIZE, SIZE, world)
            .expect("вершина позаду камери — сцена не та");
        bounds.0 = bounds.0.min(f64::from(screen[0]));
        bounds.1 = bounds.1.min(f64::from(screen[1]));
        bounds.2 = bounds.2.max(f64::from(screen[0]));
        bounds.3 = bounds.3.max(f64::from(screen[1]));
    }
    bounds
}

/// Силует асета в кадрі — це проєкція його ж вершин.
///
/// Ловить усе, заради чого крок існує: не прочитані вершини, забутий поділ
/// на довжину моделі (модель 3 м, корабель 8 м — числа різні навмисно),
/// переставлені осі й меш, який лишився заглушкою.
#[test]
fn the_asset_fills_exactly_the_pixels_its_own_projection_says() {
    let Some(gpu) = gpu() else { return };
    let model = model();
    let scene = scene_with([1.0, 0.0, 0.0, 0.0], model.extent);
    let shot = take(&gpu, &scene, Some(&model));

    let (x0, y0, x1, y1) = lit_bounds(&shot).expect("у кадрі порожньо — корабля немає");
    let expected = projected_bounds(&scene.camera, &model);
    println!("  кадр {x0},{y0} … {x1},{y1}");
    println!("  проєкція {expected:?}");

    // Допуск той самий, що у V2, і з тієї самої причини: піксель фарбується
    // по центру, тож біля вістря останній піксель не набирається, а назовні
    // за крайню вершину силует вийти не може взагалі.
    let inside = |what: &str, drawn: f64, want: f64, sign: f64| {
        let over = sign * (drawn - want);
        assert!(
            over <= 1.0,
            "{what}: кадр вийшов за проєкцію на {over} px ({drawn} проти {want})"
        );
        assert!(
            over >= -2.5,
            "{what}: кадр не дотягнув до проєкції {} px ({drawn} проти {want})",
            -over
        );
    };
    inside("ліворуч", f64::from(x0), expected.0, -1.0);
    inside("вгорі", f64::from(y0), expected.1, -1.0);
    inside("праворуч", f64::from(x1), expected.2, 1.0);
    inside("внизу", f64::from(y1), expected.3, 1.0);
}

/// Асет справді змінив кадр, а не приїхав і був проігнорований.
///
/// Заглушка V1 і клин займають різні пікселі, і різниця мусить бути великою:
/// «кадри не бітово однакові» пройшло б і на зміні одного пікселя.
#[test]
fn loading_a_model_changes_what_is_drawn() {
    let Some(gpu) = gpu() else { return };
    let model = model();
    let scene = scene_with([1.0, 0.0, 0.0, 0.0], model.extent);

    let stub = take(&gpu, &scene, None);
    let asset = take(&gpu, &scene, Some(&model));

    let mut differ = 0;
    let mut drawn = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let a = stub.pixel(x, y);
            let b = asset.pixel(x, y);
            if [a[0], a[1], a[2]] != frame::CLEAR_BYTES || [b[0], b[1], b[2]] != frame::CLEAR_BYTES
            {
                drawn += 1;
                if a != b {
                    differ += 1;
                }
            }
        }
    }
    let share = f64::from(differ) / f64::from(drawn.max(1));
    println!("  різних пікселів: {differ} з {drawn} ({share:.3})");
    assert!(share > 0.3, "асет майже не змінив кадр: {share:.3}");
}

/// Модель повертається разом із кораблем.
///
/// Клин несиметричний за всіма осями, тож жоден поворот, крім тотожного, не
/// лишає силует на місці — те саме твердження, що V1/V4 перевіряють для
/// заглушки. На кулі воно мусило б падати, і в цьому його зміст.
#[test]
fn turning_the_ship_turns_the_asset() {
    let Some(gpu) = gpu() else { return };
    let model = model();

    let upright = take(
        &gpu,
        &scene_with([1.0, 0.0, 0.0, 0.0], model.extent),
        Some(&model),
    );
    let half = std::f64::consts::FRAC_PI_4;
    let turned = take(
        &gpu,
        &scene_with([half.cos(), half.sin(), 0.0, 0.0], model.extent),
        Some(&model),
    );

    let a = lit_bounds(&upright).expect("силует");
    let b = lit_bounds(&turned).expect("силует");
    println!("  прямо {a:?}, повернутий {b:?}");
    assert_ne!(a, b, "поворот не змінив силует — орієнтації не видно");

    let mut moved = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if upright.pixel(x, y) != turned.pixel(x, y) {
                moved += 1;
            }
        }
    }
    println!("  зрушених пікселів: {moved}");
    assert!(moved > 500, "поворот зрушив лише {moved} пікселів");
}
