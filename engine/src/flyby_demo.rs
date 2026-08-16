//! Проліт повз Місяць по еліптичній орбіті — анімація (зонд етапу T).
//!
//! Той самий жанр, що [`crate::ship_demo`] і [`crate::moon_demo`], і з тієї
//! самої причини: показати **тим самим** [`Frame`], який іде у вікно, те, що
//! етап щойно зробив можливим. Тут воно показано разом, бо разом його ще
//! ніде не було видно:
//!
//! - колір поверхні з мозаїки LROC WAC і рельєф LOLA під самим кораблем;
//! - корпус із Blender (T5d) з GGX-матеріалом металу (T5c);
//! - **сяйво Місяця на тіньовий бік корпусу** (T6): воно міняється і з
//!   висотою — форм-фактор диска падає як `sin²θ`, — і з тим, над чим
//!   корабель летить, бо альбедо береться з асета, а море темніше за материк
//!   уп'ятеро;
//! - тонмапер (T5c3): у перигеї відблиск на корпусі виходить далеко за
//!   одиницю й без нього злипся б у білу пляму.
//!
//!     cargo run --release -p engine -- --flyby-demo build/flyby.apng
//!
//! ## Орбіта — точний Кеплер, а не інтегратор гри
//!
//! Апогей 6000 км, перигей 200 км над поверхнею. Задача двох тіл має замкнену
//! форму, тож траєкторія тут **точна**, а не порахована: рівняння Кеплера
//! розв'язується Ньютоном до 10⁻¹⁴.
//!
//! Це свідомо не `prop_run`. Зонд показує **кадр**, і брати для нього
//! інтегратор означало б тягнути в анімацію ефемериду, рухомий Місяць і
//! вибір фрейму — тобто три речі, жодна з яких на картинку не впливає.
//! Оракул інтегратора живе окремо (`tests/live.rs`, `--live-probe`), і
//! підмінювати його анімацією не можна: анімація не перевіряє нічого.
//!
//! ⚠ **Час у кадрах не рівномірний, і читати анімацію як швидкість не
//! можна.** Семпли беруться рівномірно за **ексцентричною аномалією**, а не
//! за часом: за часом апарат проводить більшу частину періоду біля апогею,
//! і три чверті анімації були б повільним дрейфом. При рівномірній `E` крок
//! часу біля перигею найменший, тобто найцікавіше місце показане
//! найповільніше. Той самий вибір, що геометричне зниження в `moon_demo`.

use std::path::Path;

use crate::chase::{self, Chase};
use crate::frame::Frame;
use crate::gpu::Gpu;
use crate::scene::{Body, Scene, Ship, TerrainId, TileSet};
use crate::{demo, ship, ship_demo, shot, tiles};

/// Радіус Місяця, метри — той самий, що в решті зондів.
const RADIUS_M: f64 = 1_737_400.0;

/// Гравітаційний параметр Місяця, м³/с² (DE440).
const MU: f64 = 4.902_800_118e12;

/// Висоти апогею й перигею над поверхнею, метри.
const APOAPSIS_M: f64 = 6_000_000.0;
const PERIAPSIS_M: f64 = 200_000.0;

/// Нахилення орбіти до площини `xy`, радіани.
///
/// Не нуль і не 90°: полярна орбіта пройшла б над полюсами, де мозаїка WAC
/// найгірша (знята при великих кутах падіння), а екваторіальна — уздовж
/// одного пояса. Тридцять градусів проводять трасу через Море Спокою.
const INCLINATION: f64 = 0.52;

/// Скільки метрів від камери до корабля — у габаритах корпусу.
const RANGES: f64 = 3.2;

/// Кут світила від перигею, радіани. 70° — це низьке сонце над точкою
/// найнижчого прольоту, тобто найдовші тіні там, де рельєф найкраще видно.
const SOLAR_ZENITH: f64 = 1.22;

/// Нахил камери над місцевим горизонтом у перигеї й апогеї, радіани —
/// **виміряні числа, а не смак**.
///
/// Камера третьої особи завжди дивиться **на корабель**, тож Місяць потрапляє
/// в кадр лише тоді, коли корабель опиняється між ним і оком. Розвідка кута:
/// при 0.5 рад центр Місяця лежить на 47° нижче осі погляду, при 0.9 рад — на
/// 24°, а півкут кадру всього 30°.
///
/// Звідси й два різні числа: кутовий радіус диска — **12.9° в апогеї й 63.7°
/// у перигеї**, тобто те саме нахилення дає вгорі диск на краю кадру, а внизу
/// суцільну поверхню без горизонту. Нахил повзе разом з висотою: угорі
/// камера дивиться на диск, унизу — уздовж лімба.
const PITCH_LOW: f64 = 0.38;
const PITCH_HIGH: f64 = 0.9;

pub const FRAMES: u32 = 360;
pub const FPS: u16 = 60;

/// Велика піввісь і ексцентриситет із двох висот.
fn elements() -> (f64, f64) {
    let apo = RADIUS_M + APOAPSIS_M;
    let peri = RADIUS_M + PERIAPSIS_M;
    (0.5 * (apo + peri), (apo - peri) / (apo + peri))
}

/// Стан на ексцентричній аномалії `e_anomaly`: позиція й швидкість.
///
/// Замкнена форма задачі двох тіл, у площині орбіти, потім поворот на
/// нахилення. Швидкість тут потрібна не фізиці, а **орієнтації**: ніс
/// корабля дивиться вздовж неї.
fn state_at(e_anomaly: f64) -> ([f64; 3], [f64; 3]) {
    let (a, e) = elements();
    let (sin_e, cos_e) = e_anomaly.sin_cos();

    // Позиція в перифокальній системі: перигей на осі `+x`.
    let r = a * (1.0 - e * cos_e);
    let plane = [a * (cos_e - e), a * (1.0 - e * e).sqrt() * sin_e];

    // Похідна тієї самої параметризації: `dE/dt = n·a/r`, де `n = √(μ/a³)`.
    let n = (MU / (a * a * a)).sqrt();
    let rate = n * a / r;
    let speed = [-a * sin_e * rate, a * (1.0 - e * e).sqrt() * cos_e * rate];

    // Поворот площини орбіти навколо осі `x` на нахилення.
    let (sin_i, cos_i) = INCLINATION.sin_cos();
    let lift = |v: [f64; 2]| [v[0], v[1] * cos_i, v[1] * sin_i];
    (lift(plane), lift(speed))
}

/// Кватерніон `[w, x, y, z]`, що переводить корабельний `+Z` у `forward`, а
/// корабельний `+Y` — якнайближче до `up`.
///
/// Потрібен рівно тут: у `ship_demo` орієнтація складалася з двох сталих
/// поворотів, а тут вона змінюється щокадру разом зі швидкістю.
fn look_along(forward: [f64; 3], up: [f64; 3]) -> [f64; 4] {
    let normalise = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / n, v[1] / n, v[2] / n]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };

    let z = normalise(forward);
    let x = normalise(cross(up, z));
    let y = cross(z, x);

    // Стовпці матриці — образи корабельних осей у світі; далі стандартне
    // перетворення матриці в кватерніон через найбільший слід.
    let m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]];
    let trace = m[0][0] + m[1][1] + m[2][2];
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [
            0.25 / s,
            (m[2][1] - m[1][2]) * s,
            (m[0][2] - m[2][0]) * s,
            (m[1][0] - m[0][1]) * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
        [
            (m[2][1] - m[1][2]) / s,
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
        [
            (m[0][2] - m[2][0]) / s,
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
        ]
    } else {
        let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
        [
            (m[1][0] - m[0][1]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
        ]
    }
}

/// Напрямок на світило — виведений з орбіти, а не поставлений на око.
///
/// Береться в площині орбіти під кутом [`SOLAR_ZENITH`] до перигею, з боку
/// **підльоту**. Тобто корабель знижується над освітленою поверхнею, проходить
/// перигей при низькому світилі — там, де рельєф дає найдовші тіні, — і далі
/// йде до термінатора, за яким сяйво на корпусі гасне (T6). Світило,
/// поставлене прямо над перигеєм, дало б пласку поверхню без жодної тіні.
fn sun() -> [f64; 3] {
    let (periapsis, ahead) = state_at(0.0);
    let unit = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / n, v[1] / n, v[2] / n]
    };
    let p = unit(periapsis);
    let a = unit(ahead);
    let (sin_z, cos_z) = SOLAR_ZENITH.sin_cos();
    unit([
        cos_z * p[0] - sin_z * a[0],
        cos_z * p[1] - sin_z * a[1],
        cos_z * p[2] - sin_z * a[2],
    ])
}

/// Сцена на кадр номер `k` з `frames`.
///
/// Будується з нуля щокадру навмисно: сцена — це дані, і зонд, який тримав би
/// її між кадрами, перевіряв би свій кеш, а не кадр.
pub fn scene_at(k: u32, frames: u32, tiles: TileSet, extent: f64) -> Scene {
    let t = f64::from(k) / f64::from(frames.max(2));

    // Від апогею через перигей і назад до апогею: рівномірно за `E`.
    let e_anomaly = std::f64::consts::PI * (2.0 * t - 1.0);
    let (position, velocity) = state_at(e_anomaly);

    let up = {
        let n = (position[0] * position[0] + position[1] * position[1] + position[2] * position[2])
            .sqrt();
        [position[0] / n, position[1] / n, position[2] / n]
    };
    let ship = Ship {
        centre: position,
        orientation: look_along(velocity, up),
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: extent * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    };

    // Камера — та сама третя особа, що в грі (V4), і обходить корабель тим
    // самим тягненням миші. Пів оберту за анімацію: на початку Місяць за
    // кораблем, у перигеї — під ним і впоперек кадру.
    let (a, e) = elements();
    let height = (a * (1.0 - e * e_anomaly.cos()) - a * (1.0 - e)) / (2.0 * a * e);
    let pitch = PITCH_LOW + height * (PITCH_HIGH - PITCH_LOW);

    let mut chase = Chase::at_ranges(RANGES);
    chase.drag(
        (std::f64::consts::PI * t - std::f64::consts::FRAC_PI_2) / chase::RADIANS_PER_PIXEL,
        pitch / chase::RADIANS_PER_PIXEL,
    );
    let camera = chase.camera(&ship, up);

    let mut scene = Scene::new(camera);
    scene.sun = sun();
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        // Сірий, а не синій колір фікстур: без колірного асета Місяць має
        // лишитись Місяцем.
        colour: [0.55, 0.55, 0.56, 1.0],
        air: None,
    });
    scene.ships.push(ship);
    scene
}

/// Малює `frames` кадрів і складає їх в анімований PNG.
pub fn render(gpu: &Gpu, width: u32, height: u32, frames: u32, path: &Path) -> Result<(), String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let surface = load_surface(gpu, &mut frame)?;

    // Корпус з асета, якщо він скукований; інакше заглушка V1.
    let hull = ship_demo::hull();
    if let Some(model) = &hull {
        frame.load_ship(gpu, model);
    }
    let extent = hull.as_ref().map_or(ship_demo::STUB_EXTENT, |m| m.extent);

    report();

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("flyby demo"),
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
        let scene = scene_at(k, frames, TileSet::Loaded(surface), extent);
        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("flyby demo"),
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

/// Друкує елементи орбіти — щоб число в кадрі можна було звірити з числом.
fn report() {
    let (a, e) = elements();
    let period = 2.0 * std::f64::consts::PI * (a * a * a / MU).sqrt();
    let speed = |r: f64| (MU * (2.0 / r - 1.0 / a)).sqrt();
    println!(
        "орбіта: {:.0} × {:.0} км над поверхнею, a = {:.1} км, e = {:.4}",
        PERIAPSIS_M / 1000.0,
        APOAPSIS_M / 1000.0,
        a / 1000.0,
        e
    );
    println!(
        "  період {:.2} год; швидкість {:.0} м/с у перигеї, {:.0} м/с в апогеї",
        period / 3600.0,
        speed(RADIUS_M + PERIAPSIS_M),
        speed(RADIUS_M + APOAPSIS_M)
    );
}

/// Рельєф і колір Місяця з готових асетів.
fn load_surface(gpu: &Gpu, frame: &mut Frame) -> Result<TerrainId, String> {
    let bytes = std::fs::read(demo::TERRAIN_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-dem", demo::TERRAIN_ASSET))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;

    let bytes = std::fs::read(demo::COLOUR_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-colour", demo::COLOUR_ASSET))?;
    let colour = tiles::Colour::from_bytes(&bytes)?;
    frame.load_surface(gpu, &terrain, Some(&colour))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Висоти в апогеї й перигеї — ті, що замовлені.
    ///
    /// Оракул тут не «схоже на еліпс»: обидві точки мають замкнену форму,
    /// `a(1 ± e)`, і будь-яка помилка в елементах видно одразу.
    #[test]
    fn the_orbit_has_the_two_altitudes_it_promises() {
        let radius = |e_anomaly: f64| {
            let (p, _) = state_at(e_anomaly);
            (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
        };
        let peri = radius(0.0) - RADIUS_M;
        let apo = radius(std::f64::consts::PI) - RADIUS_M;
        println!("  перигей {peri:.1} м, апогей {apo:.1} м");
        assert!((peri - PERIAPSIS_M).abs() < 1.0);
        assert!((apo - APOAPSIS_M).abs() < 1.0);
    }

    /// Швидкість — справді похідна позиції, а не окрема формула.
    ///
    /// Дві незалежні дороги до одного числа: замкнена форма проти
    /// центральної різниці за часом. Вони мусять зійтися, інакше ніс корабля
    /// дивиться не туди, куди він летить.
    #[test]
    fn the_velocity_is_the_derivative_of_the_position() {
        let (a, e) = elements();
        let n = (MU / (a * a * a)).sqrt();
        // Час від ексцентричної аномалії — рівняння Кеплера.
        let time = |anomaly: f64| (anomaly - e * anomaly.sin()) / n;
        // Обернене — Ньютоном, бо різниця береться за рівними кроками часу.
        let anomaly_at = |t: f64| {
            let mut anomaly = n * t;
            for _ in 0..64 {
                let f = anomaly - e * anomaly.sin() - n * t;
                anomaly -= f / (1.0 - e * anomaly.cos());
            }
            anomaly
        };

        for probe in [0.3, 1.1, 2.4, 3.0] {
            let t0 = time(probe);
            let step = 1.0e-3;
            let (before, _) = state_at(anomaly_at(t0 - step));
            let (after, _) = state_at(anomaly_at(t0 + step));
            let (_, velocity) = state_at(probe);
            for k in 0..3 {
                let numeric = (after[k] - before[k]) / (2.0 * step);
                assert!(
                    (numeric - velocity[k]).abs() < 1e-3 * velocity[k].abs().max(1.0),
                    "аномалія {probe}, вісь {k}: {numeric} проти {}",
                    velocity[k]
                );
            }
        }
    }

    /// Ніс корабля дивиться вздовж швидкості, а «верх» — від Місяця.
    #[test]
    fn the_ship_points_where_it_flies() {
        for k in [0u32, 37, 180, 300] {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, 0.5);
            let ship = &scene.ships[0];
            let (_, velocity) = state_at(std::f64::consts::PI * (2.0 * f64::from(k) / 360.0 - 1.0));
            let r = crate::frame::rotation(ship.orientation);
            // Образ корабельного `+Z` — третій стовпець матриці повороту.
            let nose = [r[0][2], r[1][2], r[2][2]];
            let speed = (velocity[0].powi(2) + velocity[1].powi(2) + velocity[2].powi(2)).sqrt();
            let along =
                (nose[0] * velocity[0] + nose[1] * velocity[1] + nose[2] * velocity[2]) / speed;
            println!("  кадр {k}: ніс уздовж швидкості на {along:.6}");
            assert!(along > 0.999_999, "ніс дивиться не вздовж швидкості");
        }
    }
}
