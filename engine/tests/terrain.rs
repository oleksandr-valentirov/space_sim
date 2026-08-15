//! Тайл у кадрі крізь bindless (ROADMAP-PLANETS.md, R5c).
//!
//! Три твердження, і кожне ловить свою помилку.
//!
//! 1. **Висота доходить до вершини.** Кадр із рельєфом і кадр без нього
//!    різні — інакше тайл прочитаний, завантажений і проігнорований.
//! 2. **Індексація масиву працює.** Масив з одного елемента не доводить
//!    нічого: перевіряється, що **різні тайли дають різні пікселі** —
//!    поворотом камери навколо осі світла, який гладку сферу лишає тією
//!    самою, а рельєф ні.
//! 3. **Рельєф видно тінями.** Знімок на термінаторі: там, де сонце падає
//!    навскіс, схил або освітлений, або ні. Міряється різкість перепадів
//!    яскравості (повна варіація), а не кількість відтінків: на термінаторі
//!    більшість рельєфу йде в тінь, тож відтінків там стає **менше**, а
//!    перепадів між ними — набагато більше.
//!
//! Пристрій без bindless пропускає ці тести з поясненням. Це не мовчазний
//! пропуск: bindless мають усі три цілі проєкту (PROJECT.md §7), тож
//! відсутність його означає бекенд, який і так не ціль.

use dem_cook::cook::build;
use dem_cook::Grid;
use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::lod;
use engine::scene::{Body, Scene, TerrainId, TileSet};
use engine::shot::{self, Shot};
use engine::tiles::{Terrain, HALO, NODES, STORED};
use std::path::Path;

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;
const LEVELS: u32 = 4;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("ПРОПУЩЕНО: адаптер без bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

fn terrain() -> Terrain {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/lola/ldem_4.img");
    let grid = Grid::read(&path).expect("сітка LOLA мала прочитатися");
    build(&grid, LEVELS)
}

/// Місяць у кадрі: камера на висоті `altitude` над напрямком `direction`.
fn moon(direction: [f64; 3], altitude: f64, tiles: TileSet) -> Scene {
    let length = (direction.iter().map(|v| v * v).sum::<f64>()).sqrt();
    let unit = direction.map(|v| v / length);
    let distance = MOON_RADIUS_M + altitude;
    let eye = unit.map(|v| v * distance);

    // Вертикаль кадру — **вісь світла**, а не світова z. Це і є те, що робить
    // поворот навколо світла симетрією кадру: разом із оком повертається вся
    // конфігурація, тож гладка сфера мусить дати ту саму картинку. З
    // нерухомою z вона її не давала — і перша версія цього тесту нічого не
    // ловила саме тому.
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], light()));
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        air: None,
    });
    scene
}

/// Напрямок на точку з широтою й довготою в градусах.
fn towards(lat: f64, lon: f64) -> [f64; 3] {
    let (a, b) = (lat.to_radians(), lon.to_radians());
    [a.cos() * b.cos(), a.cos() * b.sin(), a.sin()]
}

/// Одиничний напрямок на джерело світла — той самий, що в кадрі.
fn light() -> [f64; 3] {
    let l = frame::LIGHT_DIR.map(f64::from);
    let n = l.iter().map(|v| v * v).sum::<f64>().sqrt();
    l.map(|v| v / n)
}

/// Напрямок під кутом `tilt` до світла, повернутий навколо нього на `turn`.
///
/// Ключова властивість: **поворот навколо осі світла лишає освітлення
/// незмінним**. Гладка сфера при цьому дає бітово ту саму картинку —
/// конфігурація «світло, око, вертикаль» переходить сама в себе. А рельєф не
/// переходить, бо він не симетричний. Саме на цьому й стоїть перевірка
/// індексації масиву.
fn around_light(tilt: f64, turn: f64) -> [f64; 3] {
    let l = light();
    // Будь-який вектор, не паралельний до `l`, дає перший орт.
    let seed = if l[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let e1 = unit(cross(l, seed));
    let e2 = cross(l, e1);

    let (c, s) = (tilt.to_radians().cos(), tilt.to_radians().sin());
    let (ct, st) = (turn.to_radians().cos(), turn.to_radians().sin());
    [0, 1, 2].map(|k| c * l[k] + s * (ct * e1[k] + st * e2[k]))
}

/// Скільки пікселів різні між двома знімками.
fn different(a: &Shot, b: &Shot) -> usize {
    let mut count = 0;
    for y in 0..a.height {
        for x in 0..a.width {
            if a.pixel(x, y) != b.pixel(x, y) {
                count += 1;
            }
        }
    }
    count
}

/// Знімок сцени з рельєфом і без нього, з тієї самої камери.
fn pair(gpu: &Gpu, direction: [f64; 3], altitude: f64) -> (Shot, Shot) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain shot"),
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

    // Один `Frame` на обидва знімки: рельєф завантажується в нього, і саме
    // тому смуга «завантажили — не намалювали» тут неможлива.
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let id = frame
        .load_terrain(gpu, &terrain())
        .expect("рельєф мав завантажитись");

    let mut take = |tiles: TileSet| {
        let scene = moon(direction, altitude, tiles);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shot"),
            });
        frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав намалюватися")
    };

    let with = take(TileSet::Loaded(id));
    let without = take(TileSet::Smooth);
    (with, without)
}

/// Висота доходить до вершини: кадр із рельєфом не такий, як без нього.
#[test]
fn the_terrain_changes_the_frame() {
    let Some(gpu) = gpu() else { return };
    // Освітлений бік, і це не дрібниця: на нічному боці освітлення стале
    // (`shade = 0.05`), тож рельєф там не видно ні з тайлом, ні без нього.
    // Перша версія цього тесту стояла над басейном Ейткен — і той лежить
    // рівно навпроти світла.
    let (with, without) = pair(&gpu, around_light(35.0, 0.0), 5.0e4);

    let moved = different(&with, &without);
    let all = (SIZE * SIZE) as usize;
    println!("  освітлений бік з 50 км: різних пікселів {moved} з {all}");
    assert!(
        moved > all / 20,
        "рельєф змінив лише {moved} пікселів з {all} — тайл або не доїхав до \
         вершинного шейдера, або доїхав нулями"
    );
}

/// Різні тайли дають різні пікселі — інакше індексація масиву нічого не варта.
///
/// Оракул — **симетрія, яку ламає лише рельєф**. Поворот камери навколо осі
/// світла лишає освітлення незмінним, тож гладка сфера з двох таких положень
/// дає ту саму картинку: симетрична поверхня, симетричне світло. Рельєф
/// симетрії не має, і його картинки розходяться.
///
/// Реалізація, у якій усі патчі читають один тайл, теж симетрична — вона дала
/// б однакові кадри так само, як гладка сфера. Тому твердження тут не «щось
/// змінилося», а «змінилося саме там, де симетрію ламає рельєф».
#[test]
fn different_tiles_give_different_pixels() {
    let Some(gpu) = gpu() else { return };

    let altitude = 2.0e5;
    let (with_a, without_a) = pair(&gpu, around_light(35.0, 0.0), altitude);
    let (with_b, without_b) = pair(&gpu, around_light(35.0, 120.0), altitude);

    let smooth_moved = different(&without_a, &without_b);
    let terrain_moved = different(&with_a, &with_b);
    let all = (SIZE * SIZE) as usize;

    println!(
        "  поворот на 120° навколо світла: гладка сфера {smooth_moved} \
         різних пікселів, рельєф {terrain_moved} з {all}"
    );

    // Гладка сфера мусить лишитися майже тією самою. Не бітово: базис камери
    // рахується через векторні добутки, і поворот дає інші останні біти.
    // Виміряно — 808 пікселів з 65536, тобто 1.2%; поріг у 2% це і закріплює.
    assert!(
        smooth_moved < all / 50,
        "гладка сфера при повороті навколо світла змінилася на {smooth_moved} \
         пікселів — тоді симетрія, на якій стоїть цей тест, не тримається"
    );
    assert!(
        terrain_moved > all / 10,
        "рельєф при повороті навколо світла змінився лише на {terrain_moved} \
         пікселів з {all} — усі патчі читають той самий тайл"
    );
}

/// Рельєф видно тінями: на термінаторі схили розсипають освітлення.
///
/// Міряється **кількістю різних відтінків**. У гладкої сфери освітлення —
/// гладка функція нормалі, тобто небагато плавних градацій; у рельєфу кожна
/// фасетка має свій нахил, і кількість відтінків стрибає. Це те саме
/// твердження, що «видно тінями», але числом, а не оком.
#[test]
fn on_the_terminator_the_relief_shows_as_shade() {
    let Some(gpu) = gpu() else { return };

    // Термінатор — це напрямок під 90° до світла: там сонце падає рівно
    // вздовж поверхні, і найменший нахил вирішує, освітлений схил чи ні.
    let (with, without) = pair(&gpu, around_light(70.0, 0.0), 1.2e6);

    // Міряється **різкість**, а не кількість відтінків. Гладка сфера має
    // плавний градієнт: сусідні пікселі різняться на одиницю-дві. У рельєфу
    // кожна фасетка має свій нахил, і на її межі яскравість стрибає. Кількість
    // відтінків тут не годиться взагалі — на термінаторі більшість рельєфу
    // йде в тінь і зливається в один рівень, тобто відтінків стає МЕНШЕ.
    let sharp = |shot: &Shot| {
        let mut count = 0usize;
        // Повна варіація яскравості по рядках: сума модулів перепадів між
        // сусідніми пікселями.
        for y in 0..shot.height {
            for x in 1..shot.width {
                let (a, b) = (shot.pixel(x - 1, y), shot.pixel(x, y));
                if [a[0], a[1], a[2]] == frame::CLEAR_BYTES
                    || [b[0], b[1], b[2]] == frame::CLEAR_BYTES
                {
                    continue;
                }
                count += usize::from(a[2].abs_diff(b[2]));
            }
        }
        count
    };

    let rough = sharp(&with);
    let smooth = sharp(&without);
    println!(
        "  повна варіація яскравості на термінаторі: рельєф {rough}, \
         гладка сфера {smooth}"
    );

    // Знімки лягають на диск: коли це колись почервоніє, дивитися буде на що.
    let out = Path::new("build/r5c");
    let _ = with.write_png(&out.join("terminator_terrain.png"));
    let _ = without.write_png(&out.join("terminator_smooth.png"));

    assert!(
        rough > smooth * 3,
        "рельєф дав варіацію {rough} проти {smooth} у гладкої — тіней від \
         нахилу не видно"
    );
}

// ---------------------------------------------------------------------------
// Підпрямокутник чужого тайла (R7a, GPU-половина)

/// Скільки рівнів має піраміда, якої патчам **не вистачає**.
const SHALLOW: u32 = 2;
/// Скільки рівнів має піраміда, у якій кожен патч має **власний** тайл.
const DEEP: u32 = 4;
/// Метрів в одиниці зберігання. Один метр: тоді одиниці зберігання й метри —
/// те саме число, і звірка на цілість нижче читається без переведення.
const UNIT_M: f32 = 1.0;
/// Сторона кадру цієї перевірки — більша за спільну [`SIZE`], і навмисно.
///
/// Рівень вибирається за екранною похибкою, тож глибину набору купує або
/// низька висота, або високий кадр. Низька коштувала б камерою всередині
/// рельєфу (перепад ±1 км), високий кадр не коштує нічого, крім зчитування.
/// Тисяча двадцять чотири піксели дають рівень 3 із двадцяти кілометрів —
/// тобто **дві** сходинки нижче за піраміду, а не одну: модуль у зсуві вікна
/// перевіряється там, де він уже не зводиться до `i % 2`.
const SUBRECT_SIZE: u32 = 1024;

/// Висота вузла — **крутий пандус** у частках грані: `4096·(x + y)` одиниць.
///
/// Три вимоги стикаються тут, і пандус — єдина форма, що вдовольняє всі три.
///
/// 1. **Білінійна вибірка мусить відтворювати поле точно**, інакше глибокий
///    тайл зберігав би округлення й бітова рівність зламалась би не з тієї
///    причини, яку тест шукає. Лінійна функція відтворюється точно.
/// 2. **Нахил мусить бути однаковий за будь-якої бази різниці.** Відколи
///    рельєф входить у вибір рівня (R7c), дві піраміди різної глибини дають
///    різні набори патчів — і тоді порівнювати кадри нема сенсу. У лінійного
///    поля градієнт від бази не залежить узагалі, а всі множники тут —
///    степені двійки, тож рівність виходить бітова, а не «майже».
/// 3. **Освітлення мусить бачити рельєф.** Перша версія цього пандуса давала
///    512 м на чверть кола, нахил 2·10⁻⁴, і не змінювала **жодного** пікселя
///    проти гладкої сфери. Тут перепад 8192 м, нахил 2.1·10⁻³ — на порядок з
///    гаком більше, і кадр міняється помітно.
///
/// Множник `128 >> level` тримає значення цілими на кожному рівні до сьомого
/// і в межах `i16` (стеля 8192).
fn ramp_units(level: u32, u: i64, v: i64) -> i16 {
    ((u + v) * i64::from(128u32 >> level)) as i16
}

/// Мілка піраміда: власні дані на обох рівнях, глибше — нічого.
///
/// Ореол тут — продовження того самого пандуса, а не отрута: відколи нахил
/// входить у вибір рівня, крайні вузли патча читають ореол **через
/// `slope_at`**, і `i16::MIN` за краєм дав би нескінченний нахил і поділ до
/// стелі. Це і є той «голосний злам», заради якого отрута лежала.
fn shallow_relief() -> Terrain {
    let mut grids = Vec::with_capacity(Terrain::count(SHALLOW));
    for level in 0..SHALLOW {
        let side = 1u32 << level;
        for _face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let mut grid = vec![0i16; STORED * STORED];
                    for a in 0..STORED {
                        for b in 0..STORED {
                            let u = i64::from(i) * SIDE as i64 + a as i64 - HALO as i64;
                            let v = i64::from(j) * SIDE as i64 + b as i64 - HALO as i64;
                            grid[a * STORED + b] = ramp_units(level, u, v);
                        }
                    }
                    grids.push(grid);
                }
            }
        }
    }
    Terrain::build(SHALLOW, MOON_RADIUS_M, UNIT_M, &grids)
}

/// Глибока піраміда **того самого поля**: кожен її тайл — це те, що
/// [`Terrain::height_m`] читає з мілкої для того самого патча.
///
/// Тобто питання, яке ставить тест, звучить так: чи прочитає GPU з мілкої
/// піраміди те саме, що CPU вже поклав у глибоку. Рівні 0 і 1 виходять
/// дослівною копією (там `height_m` бере вузол точно), рівні 2 і 3 —
/// білінійним підпрямокутником предка.
fn deep_relief(shallow: &Terrain) -> Terrain {
    let mut grids = Vec::with_capacity(Terrain::count(DEEP));
    for level in 0..DEEP {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    // Ореол — з того самого пандуса аналітично: `height_m`
                    // за край сітки не виходить свідомо, а `slope_at` там
                    // читає, і без нього нахил на краю патча збожеволів би.
                    let mut grid = vec![0i16; STORED * STORED];
                    for a in 0..STORED {
                        for b in 0..STORED {
                            let u = i64::from(i) * SIDE as i64 + a as i64 - HALO as i64;
                            let v = i64::from(j) * SIDE as i64 + b as i64 - HALO as i64;
                            grid[a * STORED + b] = ramp_units(level, u, v);
                        }
                    }
                    for a in 0..NODES {
                        for b in 0..NODES {
                            let value = shallow.height_m(&patch, a, b) / f64::from(UNIT_M);
                            // Сторож на самій конструкцію фікстури: якщо крок
                            // висоти колись перестане ділитися на ваги, тайл
                            // почне округлятись і бітова рівність нижче
                            // зламається з зовсім іншої причини.
                            assert_eq!(
                                value,
                                value.round(),
                                "{patch:?} вузол ({a}, {b}): {value} не ціле — \
                                 множник пандуса не покриває ваг вибірки"
                            );
                            grid[(a + HALO) * STORED + b + HALO] = value as i16;
                        }
                    }
                    grids.push(grid);
                }
            }
        }
    }
    Terrain::build(DEEP, MOON_RADIUS_M, UNIT_M, &grids)
}

/// **Патч, глибший за піраміду, малює ту саму поверхню, що й патч із власним
/// тайлом** (R7a).
///
/// Це та половина оракула R7a, якої на момент кроку написати не вдалось: LOD
/// не спускався глибше за нульовий рівень, тож патчів, глибших за піраміду, у
/// кадрі не виникало взагалі. Борг D13 це закрив, і перевірка стала можлива.
///
/// **Твердження — бітова рівність двох кадрів,** знятих із однієї камери на
/// двох пірамідах **одного поля висот**: мілкій, де патч читає підпрямокутник
/// предка, і глибокій, де той самий патч має власний тайл, заповнений тим, що
/// `Terrain::height_m` прочитала з мілкої. Тобто GPU звіряється не з другою
/// копією формули, а з тією самою CPU-функцією, двійником якої оголошено
/// шейдер. Округлення на цьому шляху немає взагалі
/// ([`STEP_UNITS`]), тож і допуску не треба.
///
/// Помилка, яку це ловить, — рівно та, заради якої крок робився: патч, що
/// читає тайл предка **своїми** локальними координатами, розтягнув би весь
/// тайл предка на себе, тобто повторив би рельєф у кожному патчі й розірвав
/// його на кожній межі. Жодного допуску тут не треба — така помилка міняє
/// кадр цілком.
///
/// Третій знімок, гладкий, стоїть проти протилежної підміни: два кадри, у яких
/// висота не доїхала до вершини взагалі, теж бітово рівні.
///
/// **Камер чотири, і це не запас.** З двадцяти кілометрів видно шапку в кілька
/// градусів, тобто малюється п'ять патчів із сорока — і те, чи потрапить серед
/// них патч із **несиметричним** вікном (`origin.x != origin.y`), вирішує
/// випадок. Виміряно на одній камері: перестановка `origin.x` і `origin.y` у
/// шейдері не змінила **жодного** пікселя, хоча три з п'яти намальованих
/// патчів мали різні координати вікна. Одна камера тут просто не бачить
/// половини помилок адресації.
#[test]
fn a_patch_deeper_than_the_pyramid_draws_the_same_surface() {
    let Some(gpu) = gpu() else { return };

    // Двадцять кілометрів: на цій висоті набір іде до рівня 3 при кадрі
    // 1024 px — тобто глибше за мілку піраміду й дрібніше за глибоку. Висота
    // підібрана не на око: обидві межі перевіряються нижче, і тест червоніє,
    // якщо критерій похибки колись поїде.
    let altitude = 2.0e4;

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("subrect shot"),
        size: wgpu::Extent3d {
            width: SUBRECT_SIZE,
            height: SUBRECT_SIZE,
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

    // Один кадр на всі три знімки: обидві піраміди живуть у ньому одночасно,
    // тож між знімками не міняється взагалі нічого, крім хендла.
    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let field = shallow_relief();
    let shallow = frame
        .load_terrain(&gpu, &field)
        .expect("мілка піраміда мала завантажитись");
    let deep = frame
        .load_terrain(&gpu, &deep_relief(&field))
        .expect("глибока піраміда мала завантажитись");

    let mut take = |direction: [f64; 3], tiles: TileSet| {
        let scene = moon(direction, altitude, tiles);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("subrect"),
            });
        frame.draw(
            &gpu,
            &mut encoder,
            &view,
            SUBRECT_SIZE,
            SUBRECT_SIZE,
            &scene,
        );
        shot::read_back(&gpu, encoder, &texture, SUBRECT_SIZE, SUBRECT_SIZE)
            .expect("кадр мав намалюватися")
    };

    let all = (SUBRECT_SIZE * SUBRECT_SIZE) as usize;
    let mut asymmetric = 0;

    for turn in [0.0, 90.0, 180.0, 270.0] {
        let direction = around_light(35.0, turn);
        let scene = moon(direction, altitude, TileSet::Smooth);

        // Перевірка, що перевірка не порожня. Без неї тест лишався б зеленим
        // на наборі з самих граней — тобто саме в тому стані, у якому був до
        // D13. Заразом рахуються патчі з несиметричним вікном: без жодного
        // такого рівність кадрів не сказала б нічого про самі координати.
        let selection = lod::select(
            &lod::Body::still([0.0, 0.0, 0.0], MOON_RADIUS_M),
            &scene.camera,
            lod::focal_px(frame::FOV_Y, f64::from(SUBRECT_SIZE)),
            None,
        );
        let deepest = selection
            .patches
            .iter()
            .map(|p| p.level)
            .max()
            .expect("набір не буває порожнім");
        asymmetric += selection
            .patches
            .iter()
            .filter(|p| {
                let (_, origin, _) = field.window(p);
                origin[0] != origin[1]
            })
            .count();
        assert!(
            deepest >= SHALLOW,
            "поворот {turn}°: найглибший рівень {deepest} — жоден патч не \
             виходить за мілку піраміду, тобто підпрямокутник ніде не читається"
        );
        assert!(
            deepest < DEEP,
            "поворот {turn}°: найглибший рівень {deepest} — глибока піраміда \
             теж його не накриває, і порівнювати нема з чим"
        );

        let from_parent = take(direction, TileSet::Loaded(shallow));
        let from_own = take(direction, TileSet::Loaded(deep));
        let smooth = take(direction, TileSet::Smooth);

        let moved = different(&from_parent, &smooth);
        let apart = different(&from_parent, &from_own);
        println!(
            "  поворот {turn}°: {} патчів до рівня {deepest}, проти гладкої \
             {moved} різних з {all}, проти власного тайла {apart}",
            selection.patches.len()
        );
        assert!(
            moved > all / 20,
            "поворот {turn}°: рельєф змінив лише {moved} пікселів з {all} — \
             висота не доїхала до вершини, і рівність нижче нічого не значила б"
        );
        assert_eq!(
            apart, 0,
            "поворот {turn}°: патч глибший за піраміду намалював не ту \
             поверхню, що патч із власним тайлом — вікно в тайлі предка стоїть \
             не там"
        );
    }

    println!("  патчів із несиметричним вікном на чотирьох камерах: {asymmetric}");
    assert!(
        asymmetric > 0,
        "жодна з камер не дала патча, у якого зсув вікна різний по осях — \
         перестановка координат пройшла б непоміченою"
    );
}

/// Порожній рельєф — це помилка, а не гладка планета.
///
/// Хендл, якого немає, не має тихо перетворюватись на `Smooth`: планета без
/// гір і планета, чий асет не завантажився, виглядають однаково.
#[test]
fn a_terrain_that_does_not_fit_is_refused_out_loud() {
    let Some(gpu) = gpu() else { return };
    let mut frame = Frame::new(&gpu, shot::FORMAT);

    // Піраміда, більша за стелю масиву: 7 рівнів це 32766 тайлів.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/lola/ldem_4.img");
    let grid = Grid::read(&path).expect("сітка LOLA мала прочитатися");
    let huge = build(&grid, 7);
    let refused = frame.load_terrain(&gpu, &huge);
    println!("  завелика піраміда: {refused:?}");
    assert!(refused.is_err(), "завеликий рельєф прийняли мовчки");

    // А неіснуючий хендл у сцені просто не малює рельєфу — але й не падає.
    let scene = moon(towards(0.0, 0.0), 1.0e5, TileSet::Loaded(TerrainId(42)));
    let taken = shot::take_scene(&gpu, 64, 64, &scene);
    assert!(taken.is_ok(), "чужий хендл повалив кадр");
}
