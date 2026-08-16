//! Правило матеріалу в кадрі (етап T, крок T4b).
//!
//! Двійник у шейдері перевіряється так само, як `engine::cull` проти
//! `cull.slang`: числом, а не оком. Але кадр — не компут, і прочитати з нього
//! множник напряму не можна, тож фікстура будується так, щоб усе інше в
//! яскравості було відоме наперед.
//!
//! **Рампа зі сталим нахилом.** Тайлсет лінійний за частками грані, тож
//! `slope_at` дає в кожному вузлі те саме число (це доведено окремо —
//! `tiles::tests::the_slope_of_a_ramp_is_the_ramp`). Отже й множник
//! [`material::tint`] сталий по всьому диску, і його можна порахувати на CPU
//! до єдиного знімка.
//!
//! **Камера так далеко, що процедурної деталі немає взагалі.** Не «мало», а
//! рівно нуль: на 3·10⁵ м найгрубіша октава займає 2.5 пікселя при
//! [`detail::FADE_LO_PX`] = 4, тобто `octave_weight` повертає нуль і цикл
//! обривається на першій же. Тому в множнику лишається сам доданок нахилу.
//!
//! **Колір — стала.** Уся різниця яскравості між двома знімками тоді
//! належить множнику й нахилу фасетки, а не мозаїці.
//!
//! Лишається одна домішка, і вона рахується, а не відкидається: рампа нахиляє
//! поверхню на `atan(slope)`, тобто змінює дифузний член. При нахилі 0.12 це
//! 6.8° і 0.68% яскравості проти 24% від самого правила.
//!
//! ⚠ **Байти знімка декодуються перед будь-яким діленням** (T5a). Ціль кодує
//! гамму, тож відношення байтів — це не відношення яскравостей, а допуск «одна
//! одиниця» не має сенсу: крок байта коштує біля світлих тонів утричі більше,
//! ніж біля темних. Допуски тут виражені через [`byte_quantum`], і це не
//! перестраховка — поле в кадрі майже стале, тож усі пікселі округляються в
//! один бік і похибка не усереднюється жодною кількістю пікселів.

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::tiles::{self, Colour, Terrain, HALO, STORED};
use engine::{detail, material, srgb};

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;
/// Рівнів у пірамідах фікстури.
///
/// Два числа, а не одне: крок T4 обіцяє, що глибина піраміди в колір не
/// входить, і перевірити це можна лише двома пірамідами того самого рельєфу.
const LEVELS: u32 = 3;
const OTHER_LEVELS: u32 = 2;

/// Нахил рампи.
///
/// Затиснутий з обох боків. **Знизу** — квантуванням: знімок восьмибітний, і
/// один крок байта коштує близько відсотка яскравості, тож сигнал у 5% лишав
/// на вимір лише вчетверо більше за похибку. **Згори** — домішкою: рампа
/// нахиляє й саму поверхню, а разом з нею дифузний член. 0.12 дає сигнал 24%
/// проти домішки 0.68% і кванта 1.0%.
const SLOPE: f64 = 0.12;

/// Метрів в одиниці зберігання.
///
/// Не одиниця: сталий нахил по всьому тілу неминуче накопичує рельєф
/// (0.12 на чверть великого кола — це 327 км), і в `i16` він влазить лише з
/// грубою шкалою. Це властивість фікстури, а не тіла.
const SCALE_M: f32 = 16.0;

/// Яскравість сталого кольору, одиниці зберігання.
const FLAT_COLOUR: u8 = 160;

/// Висота, з якої видно процедурний рельєф.
///
/// Згори затиснута затуханням: найгрубіша октава (3393 м) мусить займати
/// більше за [`detail::FADE_LO_PX`] пікселів, тобто камера нижча за 188 км.
/// Чотири кілометри лишають живими п'ять октав із шести.
const NEAR_ALTITUDE: f64 = 4.0e3;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("ПРОПУЩЕНО: адаптер без bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

/// Одиниць висоти на частку грані вздовж `y`; уздовж `x` удвічі менше.
///
/// Виводиться з бажаного нахилу назад: `slope = √(g² + (2g)²) · scale / (π/2 · R)`.
fn gradient() -> f64 {
    SLOPE * std::f64::consts::FRAC_PI_2 * MOON_RADIUS_M / (5f64.sqrt() * f64::from(SCALE_M))
}

/// Рампа, лінійна за частками грані: `g·x + 2g·y`, зсунута в нуль під камерою.
///
/// Той самий вигляд, що у фікстури `tiles::tests::ramp`, і з тієї ж причини:
/// нахил такої сітки відомий аналітично й не залежить ні від рівня, ні від
/// вузла, ні від того, чи патч глибший за піраміду.
///
/// ⚠ **Відняте стале — не косметика, і без нього фікстура тиха й неправильна.**
/// Сталий нахил по всьому тілу накопичує рельєф, і під вибраним вузлом рампа
/// стояла на п'єдесталі в 98 км. Дві речі в кадрі міряються від **опорної
/// сфери**, а не від ґрунту: `Frame::near_for` (тобто ближня площина) і
/// `distance` у шейдері, яке береться по незсунутій вершині. Тому камера,
/// піднята на 30 км над ґрунтом, для затухання октав виглядала як камера на
/// 128 км — і процедурної деталі в кадрі не лишалось майже нічого. Помилка
/// виглядала як «правило не працює».
fn ramp(levels: u32) -> Terrain {
    ramp_at_sea(levels, tiles::NO_SEA)
}

/// Та сама рампа з наперед заданим рівнем моря (T7f).
///
/// Два тайлсети з неї різняться **рівно одним словом заголовка**, тож усе, чим
/// різняться їхні кадри, є множником матеріалу: геометрія, нахил і деталь у них
/// бітово одні.
fn ramp_at_sea(levels: u32, sea_units: f32) -> Terrain {
    let g = gradient();
    // Значення рампи у вузлі, над яким стоїть камера.
    let pedestal = g * (VIEW_X + 2.0 * VIEW_Y);
    let mut grids = Vec::with_capacity(Terrain::count(levels));
    for level in 0..levels {
        let side = 1u32 << level;
        for _face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let span = f64::from(SIDE as u32 * side);
                    let mut grid = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED {
                        for b in 0..STORED {
                            let a = a as isize - HALO as isize;
                            let b = b as isize - HALO as isize;
                            let x = (i as isize * SIDE as isize + a) as f64 / span;
                            let y = (j as isize * SIDE as isize + b) as f64 / span;
                            grid.push((g * x + 2.0 * g * y - pedestal) as i16);
                        }
                    }
                    grids.push(grid);
                }
            }
        }
    }
    Terrain::build(levels, MOON_RADIUS_M, SCALE_M, sea_units, &grids)
}

/// Пласкі нулі: нахил нуль, деталь нуль, множник рівно одиниця.
fn flat() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(LEVELS)];
    Terrain::build(LEVELS, MOON_RADIUS_M, SCALE_M, tiles::NO_SEA, &grids)
}

/// Колір — та сама стала скрізь, включно з ореолом.
///
/// Індикатора в ореолі тут немає навмисно, на відміну від `colour_tiles.rs`:
/// той тест питає про адресацію, а цей про яскравість, і будь-яка неоднорідність
/// мозаїки була б домішкою до самого виміру.
fn plain_colour(levels: u32) -> Colour {
    let grids = vec![vec![FLAT_COLOUR; STORED * STORED]; tiles::count(levels)];
    Colour::build(levels, 1, 0.25, false, &grids)
}

/// Вузол, над яким стоїть камера.
///
/// Не центр грані, і це не смак: уся фікстура рушія колись стояла рівно над
/// ним — єдиною точкою, де хибна геометрія дає правильну відповідь, — і D13 з
/// D14 прожили в ній невидимими. Частки грані тут 0.40 і 0.60, тобто ±9° від
/// центра; шов граней при цьому лишається за краєм кадру.
///
/// Вузол, а не довільний напрямок, з іншої причини: для нього відома **висота
/// рельєфу** (`Terrain::height_m`), а без неї камеру нема від чого відлічувати
/// — рампа піднімає поверхню на сотню кілометрів.
fn view_patch() -> Patch {
    Patch {
        face: 0,
        level: 2,
        i: 1,
        j: 2,
    }
}
const VIEW_A: usize = 19;
const VIEW_B: usize = 13;
/// Ті самі координати в частках грані — `(i·SIDE + a) / (SIDE·2^level)`.
const VIEW_X: f64 = (1.0 * 32.0 + 19.0) / (32.0 * 4.0);
const VIEW_Y: f64 = (2.0 * 32.0 + 13.0) / (32.0 * 4.0);

/// Одиничний напрямок на цей вузол.
fn view_unit() -> [f64; 3] {
    let v = view_patch().vertex(VIEW_A, VIEW_B, 1.0);
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// Місяць, освітлений точно з боку камери.
///
/// Світло вздовж погляду означає, що дифузний член на диску майже сталий, і
/// різниця між двома знімками належить множнику матеріалу, а не косинусу.
///
/// `altitude` відлічується від **поверхні під камерою**, не від опорного
/// радіуса: інакше на близькій висоті камера опинилася б усередині рампи.
fn scene(tiles: TileSet, terrain: &Terrain, altitude: f64) -> Scene {
    let unit = view_unit();
    let ground = terrain.height_m(&view_patch(), VIEW_A, VIEW_B);
    let distance = MOON_RADIUS_M + ground + altitude;
    let eye = [unit[0] * distance, unit[1] * distance, unit[2] * distance];
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    scene.sun = unit;
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

/// Яскравість пікселя в **лінійному світлі**, або `None` для порожнього неба.
///
/// ⚠ Байт із знімка декодується, і без цього весь модуль брехав би. З T5a ціль
/// кодує гамму, тож відношення двох байтів — це відношення двох гамма-кодованих
/// чисел, а множник матеріалу лінійний за побудовою. Шкала лишається 0…255
/// лише щоб числа в повідомленнях були впізнавані.
fn lit(shot: &Shot, x: u32, y: u32) -> Option<f64> {
    let p = shot.pixel(x, y);
    if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
        return None;
    }
    let mean =
        (srgb::byte_to_linear(p[0]) + srgb::byte_to_linear(p[1]) + srgb::byte_to_linear(p[2]))
            / 3.0;
    Some(mean * 255.0)
}

/// Ширина одного байта знімка в тих самих лінійних одиницях, що [`lit`].
///
/// ⚠ Потрібна саме тому, що ціль кодує гамму (T5a): один крок байта коштує
/// біля темних тонів утричі менше, ніж біля світлих, тож допуск «одна одиниця
/// яскравості» більше не має сенсу. І він не усереднюється: там, де поле в
/// кадрі стале, всі пікселі округляються **однаково**, скільки б їх не було.
fn byte_quantum(value: f64) -> f64 {
    let byte = srgb::linear_to_byte(value / 255.0);
    let up = srgb::byte_to_linear(byte.saturating_add(1));
    let down = srgb::byte_to_linear(byte.saturating_sub(1));
    (up - down) / 2.0 * 255.0
}

/// Середня яскравість і розкид у центральному вікні кадру.
///
/// Вікно, а не весь диск: біля лімба косинус падає, і будь-яке порівняння там
/// міряло б геометрію. У центрі поверхня звернена до камери й до світила
/// однаково в обох знімках.
fn window(shot: &Shot, half: u32) -> (f64, f64) {
    let mid = SIZE / 2;
    let mut values = Vec::new();
    for y in mid - half..mid + half {
        for x in mid - half..mid + half {
            if let Some(value) = lit(shot, x, y) {
                values.push(value);
            }
        }
    }
    assert!(
        values.len() > (2 * half * 2 * half) as usize * 9 / 10,
        "центр кадру не накритий поверхнею: {} з {}",
        values.len(),
        4 * half * half
    );
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let spread = values.iter().map(|v| (v - mean).abs()).sum::<f64>() / values.len() as f64;
    (mean, spread)
}

/// Знімок сцени з готовим рельєфом і сталим кольором.
fn take(gpu: &Gpu, terrain: &Terrain, altitude: f64) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material shot"),
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
    let id = frame
        .load_surface(gpu, terrain, Some(&plain_colour(terrain.levels)))
        .expect("поверхня мала завантажитись");
    let scene = scene(TileSet::Loaded(id), terrain, altitude);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("material shot"),
        });
    frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);
    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав намалюватися")
}

/// Множник з кадру збігається з множником з CPU-двійника.
///
/// Це і є перевірка того, що дві копії правила — в `engine::material` і в
/// `patch.slang` — не розійшлися.
#[test]
fn the_frame_shows_the_multiplier_the_rule_predicts() {
    let Some(gpu) = gpu() else { return };
    const ALTITUDE: f64 = 3.0e5;

    // Деталь на цій висоті мусить бути рівно нульова — інакше передбачення
    // неповне. Перевіряється, а не припускається.
    let focal = f64::from(SIZE) / 2.0 / (30f64.to_radians()).tan();
    let base = detail::base_m(MOON_RADIUS_M);
    let weight = detail::octave_weight(base, ALTITUDE, focal);
    assert_eq!(
        weight, 0.0,
        "на {ALTITUDE:.0} м деталь ще жива: вага {weight}"
    );

    let tint = material::tint(SLOPE, 0.0, false);
    // Дифузна домішка: рампа нахиляє фасетку на `atan(slope)`, і освітлення
    // в шейдері — `0.05 + 0.95·cos`.
    let cos = 1.0 / (1.0 + SLOPE * SLOPE).sqrt();
    let predicted = tint * (0.05 + 0.95 * cos) / (0.05 + 0.95);
    assert!(
        (predicted - 1.0).abs() > 0.04,
        "фікстура беззуба: правило змінює яскравість лише на {:.3}%",
        (predicted - 1.0) * 100.0
    );

    let (sloped, _) = window(&take(&gpu, &ramp(LEVELS), ALTITUDE), 32);
    let (level, _) = window(&take(&gpu, &flat(), ALTITUDE), 32);
    let measured = sloped / level;
    println!(
        "  нахил {SLOPE}: множник {tint:.4}, з дифузною домішкою {predicted:.4}, \
         у кадрі {measured:.4} ({level:.1} → {sloped:.1} одиниць)"
    );

    // Допуск — не смак, а квантування: обидва знімки стоять на майже сталій
    // яскравості, тож кожен округляється цілком в один бік, і різниця двох
    // округлень дає рівно цю межу.
    let tolerance = (byte_quantum(sloped) / sloped + byte_quantum(level) / level) / 2.0;
    println!("  допуск від кванта байта: {:.3}%", tolerance * 100.0);
    assert!(
        (measured / predicted - 1.0).abs() < tolerance,
        "кадр дав {measured:.4} проти передбачених {predicted:.4} при допуску \
         {tolerance:.4} — правило в шейдері розійшлося з `engine::material`"
    );
}

/// Під водою правило вимкнене — і вимкнене рівно, а не «майже» (T7f).
///
/// Та сама рампа, той самий кадр, різниця в заголовку тайлсета одна: рівень
/// моря вище за будь-яку висоту, яку можна записати в `i16`. Отже нахил,
/// геометрія й деталь лишились тими самими, і все, що могло змінитися, — це
/// множник.
///
/// Навіщо: правило підсвічує схил, а під водою в кадрі видно поверхню моря, а
/// не схил дна. На Землі це не дрібниця — виміряно (`--example
/// slope_histogram assets/earth.dem`), що дно **крутіше** за сушу: медіана
/// 0.0071 проти 0.0030, дев'яностий процентиль 0.0333 проти 0.0201. Без цієї
/// гілки правило малювало б серединні хребти поверх рівної води, і яскравіше,
/// ніж гори на суходолі.
#[test]
fn under_water_the_rule_does_nothing() {
    let Some(gpu) = gpu() else { return };
    const ALTITUDE: f64 = 3.0e5;

    // Та сама дифузна домішка, що й у сусіднього тесту, але **без** множника:
    // під водою він мусить бути рівно одиницею.
    let cos = 1.0 / (1.0 + SLOPE * SLOPE).sqrt();
    let predicted = (0.05 + 0.95 * cos) / (0.05 + 0.95);

    let drowned = ramp_at_sea(LEVELS, f32::from(i16::MAX));
    let (sunk, _) = window(&take(&gpu, &drowned, ALTITUDE), 32);
    let (dry, _) = window(&take(&gpu, &ramp(LEVELS), ALTITUDE), 32);
    let (level, _) = window(&take(&gpu, &flat(), ALTITUDE), 32);
    let measured = sunk / level;
    println!(
        "  під водою {measured:.4} проти передбачених {predicted:.4}; \
         над водою {:.4}",
        dry / level
    );

    // Спершу — що фікстура взагалі щось міряє: сухий і затоплений кадри мусять
    // розійтися. Інакше цей тест проходив би й тоді, коли правило вимкнене
    // скрізь.
    assert!(
        (dry - sunk).abs() > byte_quantum(dry),
        "сухий і затоплений кадри однакові ({dry:.1} проти {sunk:.1}): \
         фікстура не розрізняє гілок"
    );

    let tolerance = (byte_quantum(sunk) / sunk + byte_quantum(level) / level) / 2.0;
    assert!(
        (measured / predicted - 1.0).abs() < tolerance,
        "під водою кадр дав {measured:.4} проти {predicted:.4} при допуску \
         {tolerance:.4} — правило не вимкнулось"
    );
}

/// Рельєф доходить до кольору, і доходить лише зблизька.
///
/// Далекий кадр рампи рівний: множник сталий, бо деталі немає. Близький — ні:
/// процедурний рельєф дає ±7% яскравості. При цьому **геометрична** домішка
/// тут мізерна за побудовою: власний нахил деталі — `STEEPNESS · slope`, тобто
/// 1.4°, і затінення від неї не дотягує й до відсотка.
#[test]
fn the_relief_paints_only_when_the_camera_is_close() {
    let Some(gpu) = gpu() else { return };

    let (_, far) = window(&take(&gpu, &ramp(LEVELS), 3.0e5), 32);
    let (_, near) = window(&take(&gpu, &ramp(LEVELS), NEAR_ALTITUDE), 32);
    println!("  розкид: здалеку {far:.2}, зблизька {near:.2} одиниці");

    assert!(
        far < 1.5,
        "далекий кадр мав бути рівним, а розкид {far:.2} одиниці"
    );
    assert!(
        near > 4.0 * far.max(0.5),
        "зблизька розкид лише {near:.2} проти {far:.2} — рельєф не фарбує"
    );
}

/// Числа правила записані двічі — у Rust і в шейдері, — і мусять збігатися.
///
/// Той самий сторож, що звіряє `SIDE` у `gpu_driven.rs`: спільної константи
/// між Rust і Slang не існує, тож єдине, що лишається, — прочитати файл
/// шейдера й порівняти рядок. Помилка тут не падає й не попереджає: вона
/// малює трохи інший колір.
#[test]
fn the_shader_carries_the_same_numbers() {
    let source = include_str!("../shaders/patch.slang");
    for (name, value) in [
        ("SLOPE_GAIN", material::SLOPE_GAIN),
        ("SLOPE_REF", material::SLOPE_REF),
        ("RELIEF_GAIN", material::RELIEF_GAIN),
        ("MIN_TINT", material::MIN_TINT),
        ("MAX_TINT", material::MAX_TINT),
    ] {
        let wanted = format!("static const float {name} = {value:.2};");
        assert!(
            source.contains(&wanted),
            "у shaders/patch.slang немає рядка «{wanted}» — правило матеріалу \
             розійшлося з `engine::material`"
        );
    }
}

/// Глибина піраміди кольору не перефарбовує схил.
///
/// Це і є перевірка, названа для T4 наперед: колір мусить бути функцією
/// позиції на тілі й тільки її, тож перекукування ассета з іншою кількістю
/// рівнів не має права змінити кадр. Рампа лінійна, тож обидві піраміди
/// описують **ту саму поверхню** — грубіша просто рідшою сіткою, а вибірка
/// між її вузлами лінійна й точна.
///
/// Помилка, яку це ловить, конкретна й перевірена зламом: довжина хвилі
/// найгрубішої октави, взята з `Terrain::step_m` замість радіуса тіла. Вона
/// виглядала б бездоганно на будь-якому одному ассеті — тест падає на 30
/// одиницях яскравості.
///
/// ⚠ Чого він **не** ловить, і це варто знати: множник на `window_step`
/// усередині правила. Патч на цій висоті глибший за обидві піраміди, тож той
/// множник — 2⁻¹⁰ в одній і 2⁻⁹ в другій; правило при цьому не розходиться, а
/// зникає, і падає натомість сусідній тест про рельєф. Оракул на «однаковість»
/// сліпий до помилок, що гасять сигнал у **обох** гілках порівняння.
#[test]
fn the_pyramid_depth_does_not_repaint_the_slope() {
    let Some(gpu) = gpu() else { return };

    let deep = take(&gpu, &ramp(LEVELS), NEAR_ALTITUDE);
    let shallow = take(&gpu, &ramp(OTHER_LEVELS), NEAR_ALTITUDE);

    let mut worst = 0.0f64;
    let mut sum = 0.0;
    let mut count = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (Some(a), Some(b)) = (lit(&deep, x, y), lit(&shallow, x, y)) else {
                continue;
            };
            worst = worst.max((a - b).abs());
            sum += (a - b).abs();
            count += 1;
        }
    }
    assert!(count > 50_000, "порівняно лише {count} пікселів");
    let mean = sum / f64::from(count);
    println!("  {count} пікселів: середня різниця {mean:.3}, найгірша {worst:.1} одиниці");

    // Межа — квант восьмибітної шкали в лінійних одиницях; правило, що читало
    // б крок піраміди, дало б тут десятки.
    let quantum = byte_quantum(sum / f64::from(count) + 175.0);
    println!("  квант байта на цій яскравості {quantum:.2}");
    assert!(
        worst <= 1.5 * quantum,
        "дві глибини піраміди дали різні кольори: до {worst:.1} одиниці при \
         кванті {quantum:.2}"
    );
}
