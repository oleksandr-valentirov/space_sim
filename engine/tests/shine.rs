//! Планета підсвічує тіньовий бік корабля (етап T, крок T6).
//!
//! Три твердження, і всі три названі в ROADMAP до того, як щось написано:
//! тіньовий бік корпусу на низькій орбіті **не чорний**, колір підсвітки —
//! це колір поверхні під ним (**над морем і над материком він різний, і це
//! число**), а на нічному боці сяйво **гасне**.
//!
//! ## Маска береться з кадру без планети
//!
//! Питання тут — про пікселі, яких **не дістає світило**: саме вони до T6
//! були рівно `[0, 0, 0]`, бо ambient у кадрі нуль (PROJECT.md §7). Знайти їх
//! можна лише в кадрі, де планети немає взагалі; класифікувати за кольором
//! не можна з тієї самої причини, з якої це довелося виправляти в
//! `tests/sun.rs`: чорний піксель корпусу й чорний піксель тіні не
//! розрізняються ніяк.
//!
//! ## Море проти материка перевіряється **поворотом тіла**
//!
//! Корабель, камера, світило й сама планета лишаються бітово тими самими —
//! міняється лише `Body::orientation`, тобто те, яка ділянка асета опиняється
//! під кораблем. Пересунути корабель уздовж поверхні було б слабшою
//! перевіркою: разом з місцем поїхали б і напрямок «вниз», і кут світила, і
//! відношення яскравостей перестало б бути відношенням відбивних здатностей.
//!
//! Побічно це єдиний оракул на **поворот у систему тіла** в `shine_of`:
//! забутий поворот лишає обидва кадри однаковими.

use engine::camera::Camera;
use engine::cubesphere::FACES;
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, Ship, TileSet};
use engine::shot::{self, Shot};
use engine::srgb;
use engine::tiles::{self, Colour, Terrain, STORED};

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;

/// Висота корабля над поверхнею, метри. Низька орбіта — саме той випадок, про
/// який крок і говорить: диск планети займає майже півсферу.
const ALTITUDE_M: f64 = 100_000.0;

/// Корабель і відстань до нього, метри.
const HEIGHT_M: f64 = 20.0;
const RANGE_M: f64 = 45.0;

/// Рівнів у пірамідах — по одному: питання тесту не про піраміду, а про те,
/// звідки береться альбедо.
const LEVELS: u32 = 1;

/// Відбивні здатності фікстури — виміряні числа Місяця, не вигадані.
///
/// Медіана мозаїки LROC WAC — 0.044, контраст море-материк — приблизно
/// 0.021 проти 0.12 (T2c). Саме вони й лежать у фікстурі, тож відношення в
/// кадрі мусить бути відношенням цих двох.
const MARE: f64 = 0.021;
const HIGHLAND: f64 = 0.12;
const SCALE: f32 = 0.25;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("ПРОПУЩЕНО: адаптер без bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

/// Світило вдень: підкорабельна точка освітлена (`cos = 0.6`).
const SUN_DAY: [f64; 3] = [0.6, 0.0, 0.8];
/// Світило вночі: те саме, дзеркально через термінатор.
const SUN_NIGHT: [f64; 3] = [-0.6, 0.0, 0.8];

/// Сцена: корабель на низькій орбіті, планета під ним (або без неї).
///
/// Камера стоїть **навскіс і знизу**: згори вона бачила б лише той бік, що
/// дивиться на світило, і питати про тіньовий бік не було б у чого.
fn scene(sun: [f64; 3], body: Option<Body>, roughness: f32, metallic: f32) -> Scene {
    let centre = [MOON_RADIUS_M + ALTITUDE_M, 0.0, 0.0];
    let eye = [
        centre[0] - RANGE_M * 0.30,
        centre[1] - RANGE_M * 0.75,
        centre[2] - RANGE_M * 0.59,
    ];
    // «Вгору» для камери — від планети: інакше кадр перевернутий, і читати
    // його оком у PNG незручно без жодної користі.
    let camera = Camera::look_at(eye, centre, [1.0, 0.0, 0.0]);

    let mut scene = Scene::new(camera);
    scene.sun = sun;
    if let Some(body) = body {
        scene.bodies.push(body);
    }
    scene.ships.push(Ship {
        centre,
        orientation: [1.0, 0.0, 0.0, 0.0],
        height_m: HEIGHT_M,
        extent_m: 0.5 * HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness,
        metallic,
    });
    scene
}

/// Планета під кораблем: гладка, свого кольору.
fn smooth(colour: [f32; 4]) -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour,
        air: None,
    }
}

/// Плаский рельєф: питання про колір, і гори лише додали б власних тіней.
fn flat() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(LEVELS)];
    Terrain::build(LEVELS, MOON_RADIUS_M, 0.5, &grids)
}

/// Асет, у якому грань `+X` — море, а грань `−X` — материк.
///
/// Стала на грань, а не карта: питання тесту — чи береться альбедо з асета й
/// чи в тому місці, — і стала відповідає на нього без жодної інтерполяції.
/// Решта граней несуть третє число, тож помилка «взяли не ту грань» дає не
/// друге зі значень, а чуже.
fn two_zones() -> Colour {
    let byte = |reflectance: f64| (reflectance / f64::from(SCALE) * 255.0).round() as u8;
    let mut grids = Vec::with_capacity(tiles::count(LEVELS));
    for face in 0..FACES {
        let value = match face {
            0 => byte(MARE),
            1 => byte(HIGHLAND),
            _ => byte(0.5 * (MARE + HIGHLAND)),
        };
        grids.push(vec![value; Colour::tile_len(1)]);
    }
    // Тайли рівня 0 — по одному на грань, у порядку граней (`tiles::index`).
    assert_eq!(grids.len(), tiles::count(LEVELS));
    Colour::build(LEVELS, 1, SCALE, false, &grids)
}

/// Знімальна: одна текстура, один кадр, скільки завгодно сцен.
struct Studio {
    gpu: Gpu,
    frame: Frame,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Studio {
    fn new(gpu: Gpu) -> Studio {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shine shot"),
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
        let frame = Frame::new(&gpu, shot::FORMAT);
        Studio {
            gpu,
            frame,
            texture,
            view,
        }
    }

    fn take(&mut self, scene: &Scene) -> Shot {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shine"),
            });
        self.frame
            .draw(&self.gpu, &mut encoder, &self.view, SIZE, SIZE, scene);
        shot::read_back(&self.gpu, encoder, &self.texture, SIZE, SIZE).expect("кадр мав вийти")
    }
}

/// Пікселі корпусу, до яких світило не дійшло, — з кадру, де немає планети.
///
/// Саме вони й були рівно чорними до T6, тож саме про них і йдеться. Разом з
/// маскою повертається її розмір: перевірка, у якої під маскою три пікселі,
/// перевіряє шум.
fn unlit_mask(studio: &mut Studio, sun: [f64; 3], roughness: f32, metallic: f32) -> Vec<bool> {
    let alone = studio.take(&scene(sun, None, roughness, metallic));
    let mut mask = vec![false; (SIZE * SIZE) as usize];
    let mut count = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = alone.pixel(x, y);
            let sky = [p[0], p[1], p[2]] == frame::CLEAR_BYTES;
            if !sky && p[0] == 0 && p[1] == 0 && p[2] == 0 {
                mask[(y * SIZE + x) as usize] = true;
                count += 1;
            }
        }
    }
    assert!(
        count > 300,
        "тіньового боку в кадрі майже немає: {count} пікселів"
    );
    mask
}

/// Середнє лінійне світло по каналах під маскою.
///
/// Лінійне, а не байти: ціль знімка — sRGB (T5a), і ділити байти означало б
/// міряти передавальну функцію замість яскравості.
fn mean_linear(shot: &Shot, mask: &[bool]) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if !mask[(y * SIZE + x) as usize] {
                continue;
            }
            let p = shot.pixel(x, y);
            for c in 0..3 {
                sum[c] += srgb::byte_to_linear(p[c]);
            }
            count += 1.0;
        }
    }
    assert!(count > 0.0);
    [sum[0] / count, sum[1] / count, sum[2] / count]
}

/// Скільки пікселів під маскою перестали бути рівно чорними.
fn fraction_lit(shot: &Shot, mask: &[bool]) -> f64 {
    let mut lit = 0.0;
    let mut count = 0.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if !mask[(y * SIZE + x) as usize] {
                continue;
            }
            let p = shot.pixel(x, y);
            count += 1.0;
            if p[0] != 0 || p[1] != 0 || p[2] != 0 {
                lit += 1.0;
            }
        }
    }
    lit / count
}

/// Тіньовий бік не чорний удень і рівно чорний уночі.
///
/// Обидві половини потрібні разом. Сама по собі перша пройшла б і на
/// «ambient 0.05», який етап T свідомо прибрав; сама по собі друга — на
/// сяйві, якого немає взагалі.
#[test]
fn the_shadow_side_lights_up_over_a_day_side_and_goes_out_over_the_night() {
    let Some(gpu) = gpu() else { return };
    let mut studio = Studio::new(gpu);
    let (roughness, metallic) = (0.35f32, 0.0f32);

    let day_mask = unlit_mask(&mut studio, SUN_DAY, roughness, metallic);
    let day = studio.take(&scene(
        SUN_DAY,
        Some(smooth([0.6, 0.6, 0.6, 1.0])),
        roughness,
        metallic,
    ));
    let lit = fraction_lit(&day, &day_mask);
    let mean = mean_linear(&day, &day_mask);
    println!("  день: освітлено {lit:.3} тіньового боку, {mean:?}");
    // Не «всі», і так і має бути: сяйво приходить знизу, тож площадки, що
    // дивляться **від** планети, лишаються рівно чорними — це та сама
    // косинусна умова, що й для світила. Виміряно 0.65 при цій камері.
    assert!(
        lit > 0.5,
        "планета під кораблем не підсвітила тіньовий бік: {lit:.3}"
    );
    assert!(
        mean.iter().all(|&v| v > 0.005),
        "підсвітка є, але її не видно: {mean:?}"
    );

    let night_mask = unlit_mask(&mut studio, SUN_NIGHT, roughness, metallic);
    let night = studio.take(&scene(
        SUN_NIGHT,
        Some(smooth([0.6, 0.6, 0.6, 1.0])),
        roughness,
        metallic,
    ));
    let lit = fraction_lit(&night, &night_mask);
    println!("  ніч: освітлено {lit:.3}");
    assert_eq!(
        lit, 0.0,
        "над нічним боком планети тіньовий бік корпусу світиться"
    );
}

/// Підсвітка несе колір планети, а не сірий.
#[test]
fn the_shine_is_the_colour_of_the_planet_below() {
    let Some(gpu) = gpu() else { return };
    let mut studio = Studio::new(gpu);
    let (roughness, metallic) = (0.35f32, 0.0f32);
    let mask = unlit_mask(&mut studio, SUN_DAY, roughness, metallic);

    let blue = studio.take(&scene(
        SUN_DAY,
        Some(smooth([0.15, 0.30, 0.90, 1.0])),
        roughness,
        metallic,
    ));
    let rust = studio.take(&scene(
        SUN_DAY,
        Some(smooth([0.90, 0.30, 0.15, 1.0])),
        roughness,
        metallic,
    ));
    let blue = mean_linear(&blue, &mask);
    let rust = mean_linear(&rust, &mask);
    println!("  синя планета {blue:?}, руда {rust:?}");

    assert!(
        blue[2] > 3.0 * blue[0],
        "над синьою планетою корпус не синій: {blue:?}"
    );
    assert!(
        rust[0] > 3.0 * rust[2],
        "над рудою планетою корпус не рудий: {rust:?}"
    );
}

/// Над морем корабель підсвічений слабше, ніж над материком, — і рівно в
/// стільки разів, у скільки різняться відбивні здатності асета.
///
/// Це і є число кроку T6. Відношення передбачається наперед, з фікстури, а не
/// з кадру: «темніше» пройшло б і на реалізації, яка бере альбедо навмання.
#[test]
fn over_the_mare_the_hull_is_dimmer_than_over_the_highland() {
    let Some(gpu) = gpu() else { return };
    let mut studio = Studio::new(gpu);
    let (roughness, metallic) = (0.35f32, 0.0f32);

    let surface = studio
        .frame
        .load_surface(&studio.gpu, &flat(), Some(&two_zones()))
        .expect("поверхня з кольором мала завантажитись");

    let mask = unlit_mask(&mut studio, SUN_DAY, roughness, metallic);

    // Обертання на 180° навколо `z` підставляє під корабель протилежну
    // грань — і більше не міняє в сцені нічого.
    let mut over = |orientation: [f64; 4]| {
        let body = Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: MOON_RADIUS_M,
            orientation,
            tiles: TileSet::Loaded(surface),
            colour: frame::COLOUR,
            air: None,
        };
        let shot = studio.take(&scene(SUN_DAY, Some(body), roughness, metallic));
        mean_linear(&shot, &mask)
    };
    let mare = over([1.0, 0.0, 0.0, 0.0]);
    let highland = over([0.0, 0.0, 0.0, 1.0]);

    let measured = highland[1] / mare[1];
    let expected = HIGHLAND / MARE;
    println!("  море {mare:?}, материк {highland:?}");
    println!("  відношення: {measured:.3} проти {expected:.3} з асета");
    assert!(
        (measured - expected).abs() < 0.1 * expected,
        "яскравість не йде за асетом: {measured:.3} проти {expected:.3}"
    );
}
