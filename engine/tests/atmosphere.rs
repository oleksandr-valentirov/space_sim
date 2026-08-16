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
    ] {
        let wanted = format!("static const uint {name} = {value}u;");
        assert!(
            source.contains(&wanted),
            "у sky.slang немає рядка «{wanted}»"
        );
    }
}
