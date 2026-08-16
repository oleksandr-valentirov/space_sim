//! Колірний тайл у кадрі крізь другий bindless-масив (етап T, крок T3b).
//!
//! Три твердження, і кожне ловить свою помилку.
//!
//! 1. **Колір доходить до пікселя.** Кадр із тайлсетом і кадр без нього різні
//!    — інакше асет прочитаний, завантажений і проігнорований.
//! 2. **Карта лежить правильним боком.** Тайлсет тут — рампа **по широті**, і
//!    вона мусить лягти в кадрі як горизонтальні смуги: яскравість міняється
//!    згори вниз і майже не міняється зліва направо. Це ловить переставлені
//!    осі тайла (`a` — рядок, а не колонка) і чужий номер тайла — обидві
//!    помилки лишають кадр правдоподібним, але не смугастим.
//! 3. **Шва між тайлами немає.** Рампа неперервна на всій сфері, тож у кадрі
//!    не має бути жодного стрибка яскравості, більшого за крок самої рампи
//!    між сусідніми пікселями. Тайл шириною 33 вузли з ореолом накриває
//!    десятки пікселів, тож забутий ореол чи зсув на пів вузла дали б там
//!    видиму лінію.
//!
//! Рельєф у всіх трьох — **пласкі нулі**. Питання тут про колір, а гори
//! домалювали б до яскравості власні тіні, тобто зробили б кожен оракул
//! нечистим.

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::srgb;
use engine::tiles::{self, Colour, Terrain, HALO, STORED};

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;

/// Рівнів у пірамідах: у висот менше, ніж у кольору — рівно як у справжніх
/// асетів (5 проти 6, T2a). Дрібніші числа тут лише щоб тест не кукав
/// тисячі тайлів.
const HEIGHT_LEVELS: u32 = 2;
const COLOUR_LEVELS: u32 = 3;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("ПРОПУЩЕНО: адаптер без bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

/// Плаский рельєф: питання тесту про колір, і гори лише заважали б.
fn flat() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(HEIGHT_LEVELS)];
    Terrain::build(HEIGHT_LEVELS, MOON_RADIUS_M, 0.5, &grids)
}

/// Ореол пофарбований у колір, якого немає в сітці — це **індикатор**.
///
/// Причина міряна, а не вигадана. Перша версія фікстури продовжувала рампу й
/// в ореол, і тоді забутий зсув на `HALO` не ловився **жодним** з трьох
/// тестів: зсув на один вузол міняє широту на 0.35°, тобто яскравість на
/// 0.6 одиниці — менше за поріг, за яким взагалі можна відрізнити шов від
/// самої рампи. Ореол, помітно яскравіший за все інше, робить ту саму помилку
/// гучною: неправильний зсув малює світлу лінію по кожному ребру патча.
///
/// Читатися він при цьому не має ніколи: у вузлі `SIDE` вага ореолу рівно
/// нульова, а далі вузлів немає.
const HALO_TRACER: u8 = 255;

/// Колір як функція **широти**: від темного на південному полюсі до світлого
/// на північному.
///
/// Функція від напрямку, а не від індексів тайла, і саме тому вона неперервна
/// на всій сфері: сусідні тайли беруть її в тій самій точці, отже дають той
/// самий байт. Тобто фікстура сама по собі не має шва, і будь-який шов у
/// кадрі — це кадр.
fn latitude_ramp() -> Colour {
    let mut grids = Vec::with_capacity(tiles::count(COLOUR_LEVELS));
    for level in 0..COLOUR_LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED as isize {
                        for b in 0..STORED as isize {
                            let (a, b) = (a - HALO as isize, b - HALO as isize);
                            let inside = |v: isize| (0..=SIDE as isize).contains(&v);
                            if !inside(a) || !inside(b) {
                                tile.push(HALO_TRACER);
                                continue;
                            }
                            let unit = patch.vertex(a as usize, b as usize, 1.0);
                            let z = unit[2] / (unit.iter().map(|v| v * v).sum::<f64>()).sqrt();
                            // 0.1 … 0.9 від полюса до полюса: краї шкали
                            // лишаються вільними, щоб квантування не впиралося
                            // в нуль чи 255 — тобто щоб сітку не можна було
                            // сплутати з ореолом.
                            tile.push((255.0 * (0.5 + 0.4 * z)) as u8);
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }
    Colour::build(COLOUR_LEVELS, 1, 0.25, false, &grids)
}

/// Місяць у кадрі, освітлений з боку камери.
///
/// Світло з ока навмисно: дифузний член тоді майже не міняється по диску, і
/// різниця яскравості в кадрі — це різниця **кольору**, а не косинуса.
fn moon(tiles: TileSet, altitude: f64) -> Scene {
    let eye = [MOON_RADIUS_M + altitude, 0.0, 0.0];
    // Вертикаль кадру — світова `+z`, тобто північ. Саме тому рампа по широті
    // мусить лягти горизонтальними смугами.
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    scene.sun = [1.0, 0.0, 0.0];
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        colour: frame::COLOUR,
        air: None,
    });
    scene
}

/// Яскравість пікселя, або `None` для порожнього неба.
fn lit(shot: &Shot, x: u32, y: u32) -> Option<f64> {
    let p = shot.pixel(x, y);
    if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
        return None;
    }
    Some((f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])) / 3.0)
}

/// Пара знімків з тієї самої камери: з кольором і без нього.
fn pair(gpu: &Gpu, altitude: f64) -> (Shot, Shot) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("colour shot"),
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

    let mut frame = Frame::new(gpu, shot::FORMAT);
    let painted = frame
        .load_surface(gpu, &flat(), Some(&latitude_ramp()))
        .expect("поверхня з кольором мала завантажитись");
    let plain = frame
        .load_surface(gpu, &flat(), None)
        .expect("поверхня без кольору мала завантажитись");

    let mut take = |id| {
        let scene = moon(TileSet::Loaded(id), altitude);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shot"),
            });
        frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав намалюватися")
    };

    (take(painted), take(plain))
}

/// Колір доходить до пікселя, і кадр без нього — інший.
#[test]
fn the_colour_changes_the_frame() {
    let Some(gpu) = gpu() else { return };
    let (with, without) = pair(&gpu, 3.0e5);

    let mut changed = 0;
    let mut surface = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if lit(&with, x, y).is_some() {
                surface += 1;
                if with.pixel(x, y) != without.pixel(x, y) {
                    changed += 1;
                }
            }
        }
    }
    println!("  {changed} з {surface} пікселів поверхні змінилися");

    assert!(surface > 1000, "диск замалий: {surface} пікселів");
    assert!(
        changed * 10 > surface * 9,
        "колір змінив лише {changed} з {surface} пікселів"
    );
}

/// Рампа по широті лягає горизонтальними смугами, а не вертикальними.
///
/// Оракул — **відношення** розкиду по вертикалі до розкиду по горизонталі.
/// Абсолютні значення тут нічого не сказали б: вони залежать і від шкали
/// кольору, і від дифузного члена, а відношення — ні.
#[test]
fn a_latitude_ramp_shows_as_horizontal_bands() {
    let Some(gpu) = gpu() else { return };
    let (with, _) = pair(&gpu, 3.0e5);

    // Середня яскравість рядка й колонки — по тих пікселях, де є поверхня.
    let mean = |along_row: bool, k: u32| {
        let mut sum = 0.0;
        let mut count = 0;
        for other in 0..SIZE {
            let (x, y) = if along_row { (other, k) } else { (k, other) };
            if let Some(value) = lit(&with, x, y) {
                sum += value;
                count += 1;
            }
        }
        (count > 20).then(|| sum / f64::from(count))
    };

    let rows: Vec<f64> = (0..SIZE).filter_map(|k| mean(true, k)).collect();
    let columns: Vec<f64> = (0..SIZE).filter_map(|k| mean(false, k)).collect();
    let spread = |values: &[f64]| {
        let lo = values.iter().cloned().fold(f64::MAX, f64::min);
        let hi = values.iter().cloned().fold(f64::MIN, f64::max);
        hi - lo
    };
    let (vertical, horizontal) = (spread(&rows), spread(&columns));
    println!("  розкид по рядках {vertical:.1}, по колонках {horizontal:.1}");

    assert!(
        vertical > 4.0 * horizontal,
        "рампа по широті дала розкид {vertical:.1} згори вниз і {horizontal:.1} \
         впоперек — карта лежить не тим боком"
    );
    // І напрямок: північ угорі кадру, тобто верхні рядки світліші за нижні.
    assert!(
        rows[0] > rows[rows.len() - 1],
        "північ вийшла темнішою за південь: {:.1} проти {:.1}",
        rows[0],
        rows[rows.len() - 1]
    );
}

/// Між тайлами немає шва: сусідні пікселі не стрибають.
///
/// Камера низько навмисно — тоді в кадрі десятки патчів, тобто десятки меж
/// тайлів, і кожна з них проходить через диск. Поріг у одиницях яскравості:
/// рампа міняється на ~0.5 одиниці на піксель при цьому масштабі, тож стрибок
/// у п'ять одиниць — це не рампа, а межа.
#[test]
fn the_tile_boundaries_leave_no_seam() {
    let Some(gpu) = gpu() else { return };
    let (with, _) = pair(&gpu, 1.0e5);

    let mut worst = 0.0f64;
    let mut jumps = 0;
    let mut pairs = 0;
    let mut surface = 0;
    for y in 1..SIZE - 1 {
        for x in 1..SIZE - 1 {
            let Some(here) = lit(&with, x, y) else {
                continue;
            };
            surface += 1;
            // Лише пікселі, чиї сусіди теж на поверхні: край диска — це
            // законний стрибок у небо, і про нього тест не питає.
            for (dx, dy) in [(1u32, 0u32), (0, 1)] {
                let Some(there) = lit(&with, x + dx, y + dy) else {
                    continue;
                };
                let jump = (here - there).abs();
                worst = worst.max(jump);
                pairs += 1;
                if jump > 5.0 {
                    jumps += 1;
                }
            }
        }
    }
    println!(
        "  поверхні {surface} пікселів, найбільший стрибок {worst:.1} одиниці, \
         {jumps} з {pairs} пар"
    );

    // Диск мусить накривати кадр: перевірка «стрибків немає» на порожньому
    // небі пройшла б бездоганно й не сказала б нічого.
    assert!(
        surface * 10 > (SIZE * SIZE) as usize * 9,
        "поверхні лише {surface} пікселів"
    );
    assert!(pairs > 5000, "перевірено лише {pairs} пар пікселів");
    assert_eq!(jumps, 0, "знайшлися {jumps} стрибків — це шов між тайлами");
}

/// Стала мозаїка: та сама одиниця зберігання в кожному вузлі.
fn plain(value: u8, scale: f32) -> Colour {
    let grids = vec![vec![value; STORED * STORED]; tiles::count(COLOUR_LEVELS)];
    Colour::build(COLOUR_LEVELS, 1, scale, false, &grids)
}

/// Піксель несе саме ту відбивну здатність, яку виміряла мозаїка (T5b).
///
/// Це найпряміший оракул етапу й перший, який взагалі став можливим: до T5b у
/// шейдері стояла заглушка `terrain.y = 1`, тобто кадр малював **одиниці
/// зберігання**, а не альбедо, і питати про фізичне число не було сенсу. Тепер
/// множник — `Colour::scale`, і весь ланцюг перевіряється одним рівнянням.
///
/// Фікстура прибирає з дороги все, крім самого альбедо:
///
/// * рельєф плаский, тож правило матеріалу дає рівно одиницю;
/// * мозаїка стала, тож вибірка й вікна нічого не додають;
/// * світло вздовж погляду, а тіло далеко — у центрі кадру нормаль дивиться
///   і в камеру, і на світило, тобто дифузний член рівно один, і множники
///   `0.05 + 0.95·cos` з шейдера в передбачення не входять узагалі.
///
/// Лишається `байт = srgb(unit · scale)` — і саме це число тест і порівнює.
#[test]
fn the_pixel_carries_the_reflectance_the_mosaic_measured() {
    let Some(gpu) = gpu() else { return };

    // Три відбивні здатності, що накривають діапазон Місяця: темне море,
    // типовий матерік, світлий промінь свіжого кратера.
    for (value, scale) in [(45u8, 0.25f32), (160, 0.25), (255, 0.25)] {
        let colour = plain(value, scale);
        let expected_reflectance = colour.reflectance(0, 0, 0, 0);
        let expected = srgb::linear_to_byte(expected_reflectance);

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflectance"),
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
        let id = frame
            .load_surface(&gpu, &flat(), Some(&colour))
            .expect("поверхня мала завантажитись");
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("reflectance"),
            });
        let scene = moon(TileSet::Loaded(id), 3.0e5);
        frame.draw(&gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        let shot = shot::read_back(&gpu, encoder, &texture, SIZE, SIZE).expect("кадр");

        let got = shot.pixel(SIZE / 2, SIZE / 2)[0];
        println!(
            "  одиниця {value} × {scale} = {expected_reflectance:.4}: чекали байт \
             {expected}, у кадрі {got}"
        );
        assert!(
            got.abs_diff(expected) <= 1,
            "відбивна здатність {expected_reflectance:.4} мала дати байт \
             {expected}, а кадр дав {got}"
        );
    }
}
