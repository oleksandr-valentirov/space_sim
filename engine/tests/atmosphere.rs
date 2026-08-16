//! Таблиця пропускання збігається з оракулом (ROADMAP-ATMOSPHERE.md, S2).
//!
//! ## Що саме тут доводиться
//!
//! Правило 2 етапу S: кожен LUT має **число**, а не «схоже на небо». Число
//! приходить з `engine::atmosphere` — окремої реалізації тієї самої фізики в
//! `f64` на CPU, — а сам оракул пришпилений замкненою формою в своїх юніт-тестах.
//! Ланцюг цілком:
//!
//! 1. `β·H·(exp(−h₀/H) − exp(−h₁/H))` ⇄ `atmosphere::optical_depth`
//!    (`engine::atmosphere::tests`, без GPU);
//! 2. `atmosphere::optical_depth` ⇄ таблиця на GPU — **тут**.
//!
//! Обидві ланки потрібні. Без першої два чисельні інтегрування зійшлися б і на
//! спільній помилці; без другої шейдер не перевірений узагалі.
//!
//! ## Чому «на десятках висот і кутів», а не в одній точці
//!
//! Помилка в параметризації дає правильне число рівно там, де `u = 0` —
//! вертикаль виходить із неї сама. Помилка в озоні видна лише в шарі 10–40 км.
//! Помилка в геометрії — лише під великим кутом, де промінь довгий. Одна точка
//! не бачить жодної з трьох.

use engine::atmosphere;
use engine::gpu::Gpu;
use engine::scene::Atmosphere;
use engine::sky::Sky;

const BOTTOM: f64 = 6_371_000.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// Таблиця на GPU й оракул на CPU дають те саме пропускання.
///
/// Порівнюються **всі** 16 384 текселі, а не вибірка: таблиця мала, а помилка,
/// що живе в одному куті, — рівно те, чого вибірка не бачить.
///
/// Допуск — на пропускання, а не на оптичну товщу, і це навмисно: у кадр іде
/// саме пропускання, тож похибка мусить міритися там, де вона впливає.
#[test]
fn the_transmittance_table_matches_the_oracle_everywhere() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    assert!(sky.ensure(&gpu, &air, BOTTOM), "перший раз таблицю рахують");

    let table = sky
        .read_transmittance(&gpu)
        .expect("таблиця мала прочитатися");
    let width = atmosphere::TRANSMITTANCE_WIDTH;
    let height = atmosphere::TRANSMITTANCE_HEIGHT;
    assert_eq!(table.len(), (width * height) as usize);

    let mut worst = 0.0f64;
    let mut worst_at = (0u32, 0u32, 0usize);
    for y in 0..height {
        for x in 0..width {
            // Кінці одиничного діапазону — у центрах крайніх текселів, як у
            // шейдері: ділиться на `розмір − 1`, а не на розмір.
            let u = f64::from(x) / f64::from(width - 1);
            let v = f64::from(y) / f64::from(height - 1);
            let (r, mu) = atmosphere::uv_to_r_mu(&air, BOTTOM, u, v);
            let expected = atmosphere::transmittance(&air, BOTTOM, r, mu, atmosphere::ORACLE_STEPS);
            let got = table[(y * width + x) as usize];
            for channel in 0..3 {
                let difference = (f64::from(got[channel]) - expected[channel]).abs();
                if difference > worst {
                    worst = difference;
                    worst_at = (x, y, channel);
                }
            }
        }
    }

    // 10⁻³ — виміряна стеля, а не кругле число з голови. Складається вона з
    // двох доданків, і більший тут не той, на який думається: крок
    // інтегрування (500 проти 2048 в оракула) дає 3.6·10⁻⁵ навіть на
    // найгіршому промені таблиці, а решту — **зберігання**: half-float має 11
    // значущих бітів, тобто крок 5·10⁻⁴ біля одиниці. Тобто таблицю обмежує
    // формат, а не арифметика, і додавати шейдеру кроків не було б чого.
    assert!(
        worst < 1.0e-3,
        "найгірша розбіжність {worst} у текселі {worst_at:?}"
    );
}

/// Перший стовпець таблиці — вертикальний промінь, і його оракул замкнений.
///
/// Окремо від тесту вище, бо тут порівняння йде **не з чисельним
/// інтегруванням узагалі**: `vertical_optical_depth` — формула. Отже шейдер
/// звіряється з арифметикою, у якій кроку інтегрування немає.
#[test]
fn the_vertical_column_matches_the_closed_form() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    sky.ensure(&gpu, &air, BOTTOM);
    let table = sky
        .read_transmittance(&gpu)
        .expect("таблиця мала прочитатися");
    let width = atmosphere::TRANSMITTANCE_WIDTH;

    let mut worst = 0.0f64;
    for y in 0..atmosphere::TRANSMITTANCE_HEIGHT {
        let v = f64::from(y) / f64::from(atmosphere::TRANSMITTANCE_HEIGHT - 1);
        let (r, mu) = atmosphere::uv_to_r_mu(&air, BOTTOM, 0.0, v);
        // Рівно вертикаль з точністю до округлення `f64`, а не «майже»:
        // заради цього кінці одиничного діапазону й сідають у центри крайніх
        // текселів. До S2 найближчий до вертикалі тексель мав `mu = 0.98`.
        assert!(
            (mu - 1.0).abs() < 1.0e-12,
            "рядок {y}: стовпець 0 має дивитися строго вгору, а mu = {mu}"
        );

        let closed = atmosphere::vertical_optical_depth(&air, BOTTOM, r);
        let got = table[(y * width) as usize];
        for channel in 0..3 {
            let expected = (-closed[channel]).exp();
            worst = worst.max((f64::from(got[channel]) - expected).abs());
        }
    }
    assert!(worst < 2.0e-3, "найгірша розбіжність із формулою {worst}");
}

/// Пропускання зростає з висотою й спадає з нахилом променя.
///
/// Дві монотонності, які не залежать від жодного числа й ловлять переплутані
/// осі таблиці — помилку, яку допуск не бачить, бо після перестановки обидва
/// боки порівняння читають той самий неправильний тексель.
#[test]
fn the_table_is_monotone_in_both_of_its_axes() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    sky.ensure(&gpu, &air, BOTTOM);
    let table = sky
        .read_transmittance(&gpu)
        .expect("таблиця мала прочитатися");
    let width = atmosphere::TRANSMITTANCE_WIDTH;
    let height = atmosphere::TRANSMITTANCE_HEIGHT;
    let at = |x: u32, y: u32| f64::from(table[(y * width + x) as usize][2]);

    // Вище — прозоріше: над головою лишається менше повітря.
    for y in 1..height {
        assert!(
            at(0, y) >= at(0, y - 1) - 1.0e-6,
            "рядок {y}: {} проти {}",
            at(0, y),
            at(0, y - 1)
        );
    }
    // Полого — темніше: промінь іде крізь довший шлях.
    for x in 1..width {
        assert!(
            at(x, height / 2) <= at(x - 1, height / 2) + 1.0e-6,
            "стовпець {x}: {} проти {}",
            at(x, height / 2),
            at(x - 1, height / 2)
        );
    }
}

/// Таблиця не перераховується, поки повітря те саме, і перераховується, коли
/// не те.
///
/// Правило 5 етапу S каже, що пропускання рахують «раз назавжди». Твердження,
/// яке легко написати в коментарі й важко помітити зламаним, — тож воно тут.
#[test]
fn the_table_is_recomputed_only_when_the_air_changes() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    assert!(sky.ensure(&gpu, &air, BOTTOM), "перший раз — рахуємо");
    assert!(
        !sky.ensure(&gpu, &air, BOTTOM),
        "те саме повітря — не рахуємо"
    );

    // Інший радіус того самого тіла — інша атмосфера: висота над поверхнею
    // рахується від нього.
    assert!(
        sky.ensure(&gpu, &air, BOTTOM + 1000.0),
        "інший радіус — інша таблиця"
    );

    let mut thicker = air;
    thicker.rayleigh_height_m *= 2.0;
    assert!(sky.ensure(&gpu, &thicker, BOTTOM + 1000.0), "інше повітря");
}

/// Розміри таблиці записані і в Rust, і в Slang — і мусять збігатися.
///
/// Спільної константи між ними не існує, тож звіряє їх сторож, який греппить
/// файл шейдера. Той самий прийом, що з `SIDE` у патчів (R6a).
#[test]
fn the_table_size_is_the_same_on_both_sides() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders/sky.slang"),
    )
    .expect("шейдер мав прочитатися");

    for (name, value) in [
        ("TRANSMITTANCE_WIDTH", atmosphere::TRANSMITTANCE_WIDTH),
        ("TRANSMITTANCE_HEIGHT", atmosphere::TRANSMITTANCE_HEIGHT),
        ("MULTISCATTER_SIZE", atmosphere::MULTISCATTER_SIZE),
        (
            "MULTISCATTER_DIRECTIONS",
            atmosphere::MULTISCATTER_DIRECTIONS,
        ),
        ("MULTISCATTER_STEPS", atmosphere::MULTISCATTER_STEPS),
    ] {
        let wanted = format!("static const uint {name} = {value}u;");
        assert!(
            source.contains(&wanted),
            "у sky.slang немає рядка «{wanted}»"
        );
    }
}

// ---------------------------------------------------------------------------
// S3 — багаторазове розсіювання
// ---------------------------------------------------------------------------

/// Таблиця й CPU-двійник дають те саме `ψ`.
///
/// Двійник читає **свою** таблицю пропускання, побудовану в `f64`, а не ту, що
/// на GPU. Тобто в порівняння входить і похибка самої таблиці, і це навмисно:
/// у кадрі шейдер теж читатиме таблицю, а не інтеграл, і перевіряти треба той
/// шлях, яким небо справді малюється.
#[test]
fn the_multiscatter_table_matches_the_oracle() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    sky.ensure(&gpu, &air, BOTTOM);
    let table = sky
        .read_multiscatter(&gpu)
        .expect("таблиця мала прочитатися");
    let size = atmosphere::MULTISCATTER_SIZE;
    assert_eq!(table.len(), (size * size) as usize);

    // Таблиця пропускання для двійника — тими самими 500 кроками, що й у
    // шейдері: тут перевіряється розсіювання, а точність пропускання вже
    // перевірена вище й окремо.
    let transmittance = atmosphere::Table::transmittance(&air, BOTTOM, 500);

    let mut worst = 0.0f64;
    let mut worst_at = (0u32, 0u32);
    let mut largest = 0.0f64;
    for y in 0..size {
        for x in 0..size {
            let u = f64::from(x) / f64::from(size - 1);
            let v = f64::from(y) / f64::from(size - 1);
            let (r, mu_s) = atmosphere::multiscatter_uv(&air, BOTTOM, u, v);
            let (psi, _) = atmosphere::multiple_scattering(&air, BOTTOM, &transmittance, r, mu_s);
            let got = table[(y * size + x) as usize];
            for channel in 0..3 {
                largest = largest.max(psi[channel]);
                let expected = psi[channel];
                let difference = (f64::from(got[channel]) - expected).abs();
                // Допуск має два доданки, бо джерел похибки два, і на різних
                // кінцях таблиці головує різне.
                //
                // 10⁻⁷ — **зберігання**. Уночі `ψ` падає до 5·10⁻⁷, тобто в
                // субнормальні half-float, де крок дорівнює 6·10⁻⁸ незалежно
                // від значення; відносного допуску там не існує в принципі.
                //
                // 1% — **арифметика**: двійник читає власну таблицю пропускання
                // в `f64`, шейдер — свою в half-float, і різниця вибірок
                // проходить крізь усі 64 напрямки. Виміряно: удень розбіжність
                // 0.1%, тобто вдесятеро менша за допуск.
                let allowed = 1.0e-7 + 0.01 * expected.max(f64::from(got[channel]));
                if difference - allowed > worst {
                    worst = difference - allowed;
                    worst_at = (x, y);
                }
            }
        }
    }

    assert!(largest > 0.0, "таблиця порожня — усе нулі");
    assert!(
        worst <= 0.0,
        "тексель {worst_at:?} виходить за допуск на {worst}"
    );
}

/// Енергія не росте: ряд розсіювань збігається скрізь.
///
/// `ψ = L₂/(1 − f)` має сенс лише при `f < 1`; при `f ≥ 1` кожне наступне
/// розсіювання додавало б не менше за попереднє, і сума розходилася б. Це і є
/// оракул кроку, названий у ROADMAP-ATMOSPHERE.md, і саме заради нього `f`
/// лежить в альфі таблиці.
#[test]
fn every_further_scattering_adds_less_than_the_one_before() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    sky.ensure(&gpu, &air, BOTTOM);
    let table = sky
        .read_multiscatter(&gpu)
        .expect("таблиця мала прочитатися");

    let mut largest_fraction = 0.0f32;
    for (index, texel) in table.iter().enumerate() {
        let fraction = texel[3];
        assert!(
            (0.0..1.0).contains(&fraction),
            "тексель {index}: частка {fraction} — ряд не збігається"
        );
        largest_fraction = largest_fraction.max(fraction);
        for (channel, value) in texel.iter().enumerate().take(3) {
            assert!(
                value.is_finite() && *value >= 0.0,
                "тексель {index}, канал {channel}: {value}"
            );
        }
    }
    // Виміряно: найбільша частка помітно менша за одиницю, тобто до межі
    // збіжності повітря Землі не підходить близько. Число тут — сторож на
    // випадок, коли хтось підніме розсіювання й тихо наблизиться до неї.
    assert!(
        largest_fraction < 0.5,
        "найбільша частка {largest_fraction} — підозріло близько до межі"
    );
}

/// Більше сонця — не менше світла; вище за пік — менше розсіяного.
///
/// Дві властивості осей, які ловлять їх перестановку: після неї збіг із
/// двійником зберігся б (обидва читають той самий неправильний тексель), а ці
/// — ні.
///
/// **Друга властивість не «монотонно спадає», і це виміряно, а не спрощено.**
/// Профіль по висоті має максимум на ~6 км: біля самої поверхні нижня півсфера
/// не світить нічим (альбедо нуль, S3), тож розсіяного там менше, ніж на
/// кілька кілометрів вище. Далі повітря кінчається, і `ψ` падає монотонно аж
/// до верхньої межі. Записати сюди «спадає скрізь» означало б підганяти
/// твердження під зручність.
#[test]
fn the_multiscatter_table_is_monotone_in_both_of_its_axes() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu);
    sky.ensure(&gpu, &air, BOTTOM);
    let table = sky
        .read_multiscatter(&gpu)
        .expect("таблиця мала прочитатися");
    let size = atmosphere::MULTISCATTER_SIZE;
    let at = |x: u32, y: u32| f64::from(table[(y * size + x) as usize][2]);

    // Сонце вище над горизонтом — розсіяного світла не менше.
    for y in 0..size {
        for x in 1..size {
            assert!(
                at(x, y) >= at(x - 1, y) * 0.999,
                "рядок {y}, стовпець {x}: {} проти {}",
                at(x, y),
                at(x - 1, y)
            );
        }
    }

    // Профіль по висоті на полудні. Він **не монотонний**, і це не шум — це
    // озон, і саме тому твердження тут таке дрібне.
    //
    // Виміряно: максимум на 6.5 км (біля самої поверхні нижня півсфера не
    // світить нічим — альбедо нуль, S3), далі спад, провал на 35 км — там
    // озоновий шар з'їдає те, що мало б розсіятись, — тоді **другий підйом** до
    // 58 км, уже над шаром, і врешті спад до верхньої межі, де розсіювати нема
    // на чому. Написати сюди «спадає з висотою» було б простіше й неправдою.
    let noon: Vec<f64> = (0..size).map(|y| at(size - 1, y)).collect();
    let peak = noon
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)
        .expect("стовпець не порожній");
    assert!(
        peak < (size / 8) as usize,
        "пік розсіювання на рядку {peak} з {size} — це вже не приземний шар"
    );

    // Провал в озоновому шарі: мінімум між 20 і 50 км нижчий і за те, що під
    // ним, і за те, що над ним.
    let layer = 6..16;
    let dip = layer
        .clone()
        .min_by(|a, b| noon[*a].total_cmp(&noon[*b]))
        .expect("діапазон не порожній");
    assert!(
        noon[dip] < noon[4] && noon[dip] < noon[18],
        "провалу в озоновому шарі немає: {} проти {} знизу й {} згори",
        noon[dip],
        noon[4],
        noon[18]
    );

    // Але повітря таки кінчається: на верхній межі розсіяного вдвічі менше, ніж
    // у піку. Це і є те, що перестановка осей зламала б.
    assert!(
        noon[(size - 1) as usize] < noon[peak] * 0.5,
        "на верхній межі {} проти піка {}",
        noon[(size - 1) as usize],
        noon[peak]
    );
}
