//! Шрифт справді має гліфи для всього, що ми пишемо (ROADMAP-UI.md, U7b).
//!
//! Питання кроку поставлене так: типові шрифти egui кирилицю, найімовірніше,
//! покривають — але **перевірити, а не припустити**. Відсутній гліф малюється
//! порожнім прямокутником, тобто помилка тиха рівно доти, доки хтось не
//! подивиться на екран. А дивитись доводиться на кожну панель окремо, бо
//! рядок з'являється лише в своєму стані.
//!
//! ## Чому це не знімок
//!
//! Знімок сказав би «щось намальовано», і на тофу він теж зелений: порожній
//! прямокутник — це теж пікселі. Питання не в тому, чи є фарба, а в тому, чи
//! **різні** символи дають **різні** растри.
//!
//! Тому оракул інший і точніший: egui для невідомого символу підставляє
//! растр заміни, один і той самий. Отже беремо символ, якого в шрифті свідомо
//! немає (приватна зона Unicode), запам'ятовуємо його кут в атласі — і жоден
//! гліф жодного нашого рядка не має права на нього збігтися.
//!
//! Це ще й дешевше: GPU тут не потрібен взагалі, тобто перевірка біжить і
//! там, де адаптера немає, а не пропускається мовчки.
//!
//! ## Чому обидві таблиці, а не лише українська
//!
//! Назви мов — ендоніми (U7a), тож «Українська» лежить і в **англійській**
//! таблиці. Кирилиця потрібна в англійському інтерфейсі так само, як в
//! українському, і перевіряти лише одну таблицю означало б перевіряти не те.

use engine::egui;

use game::text::{tr, Language, ALL};

/// Символи з приватної зони Unicode: жоден осмислений шрифт їх не несе.
///
/// Два, а не один — щоб перевірка вміла провалитися. Якщо ці двоє дадуть
/// різні растри, значить «растр заміни» — не константа, і весь оракул нижче
/// не означає нічого.
const MISSING: [char; 2] = ['\u{E000}', '\u{E001}'];

/// Кут гліфа в атласі шрифта — саме те, що відрізняє один растр від іншого.
///
/// Пара `[u16; 2]`, а не `UvRect`: сам тип із `epaint` не реекспортований, а
/// для порівняння потрібні рівно ці чотири числа.
type Raster = ([u16; 2], [u16; 2]);

/// Розкладає рядки й повертає гліфи кожного.
///
/// Усі разом і за один кадр, бо атлас шрифта будується всередині кадру:
/// `Context::fonts` до першого `run_ui` не існує взагалі, а `layout_no_wrap`
/// хоче мутабельний доступ, якого читач `fonts` не дає. Через `Painter` іде
/// той самий шлях, яким текст розкладає сама панель, — тобто перевіряється те
/// саме, що побачить гравець.
fn rasters(family: egui::FontFamily, texts: &[String]) -> Vec<Vec<(char, Raster)>> {
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(300.0, 300.0),
        )),
        ..Default::default()
    };

    let mut out = Vec::new();
    let mut output = context.run_ui(input, |ui| {
        out = texts
            .iter()
            .map(|text| {
                let galley = ui.painter().layout_no_wrap(
                    text.clone(),
                    egui::FontId::new(14.0, family.clone()),
                    egui::Color32::WHITE,
                );
                galley
                    .rows
                    .iter()
                    .flat_map(|row| row.glyphs.iter())
                    .map(|glyph| (glyph.chr, (glyph.uv_rect.min, glyph.uv_rect.max)))
                    .collect()
            })
            .collect();
    });
    // `TexturesDelta` падає в `Drop`, якщо її не застосувати (U1a).
    output.textures_delta.clear();
    out
}

/// Растр першого символу рядка.
fn one(family: egui::FontFamily, text: &str) -> (char, Raster) {
    let mut glyphs = rasters(family, &[text.to_string()]).remove(0);
    assert_eq!(glyphs.len(), 1, "{text:?} розклався не в один гліф");
    glyphs.remove(0)
}

/// Обидва сімейства, які egui дає з коробки.
///
/// Панелі сьогодні шрифту не задають, тобто беруть пропорційний. Моноширинний
/// перевіряється **наперед**: U7c про приладову палітру, а прилади хочеться
/// набрати моноширинним — і з'ясувати, що в ньому немає кирилиці, дешевше
/// зараз, ніж посеред того кроку.
fn families() -> [egui::FontFamily; 2] {
    [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
}

/// Спершу — що оракул узагалі працює: відсутнє дає растр заміни, і він один.
///
/// Без цього тест нижче був би зелений і на шрифті без жодного гліфа: якби
/// кожен невідомий символ давав власний растр, «не збігся із заміною» не
/// означало б нічого.
#[test]
fn a_missing_glyph_falls_back_to_one_and_the_same_raster() {
    for family in families() {
        let first = one(family.clone(), &MISSING[0].to_string());
        let second = one(family.clone(), &MISSING[1].to_string());

        assert_eq!(
            first.1, second.1,
            "{family:?}: два різні відсутні символи дали різні растри — значить \
             egui не підставляє заміну, і перевірка нижче не має оракула"
        );

        // І та заміна щось малює. Растр нульового розміру означав би, що
        // невідоме просто не видно, — тоді збіг вище був би збігом двох ніщо.
        assert_ne!(
            first.1 .0, first.1 .1,
            "{family:?}: растр заміни порожній — оракул порівнював би дві порожнечі"
        );
    }
}

/// А тепер головне: жоден рядок жодної таблиці не малюється заміною.
#[test]
fn every_string_in_both_tables_has_real_glyphs() {
    let mut texts = Vec::new();
    let mut labels = Vec::new();
    for key in ALL {
        for language in [Language::English, Language::Ukrainian] {
            texts.push(tr(language, key).to_string());
            labels.push(format!("{key:?} ({language:?})"));
        }
    }

    for family in families() {
        let tofu = one(family.clone(), &MISSING[0].to_string()).1;

        let mut checked = 0;
        for (glyphs, label) in rasters(family.clone(), &texts).iter().zip(&labels) {
            for (chr, raster) in glyphs {
                // Пробіл законно не має растру, і порівнювати його із заміною
                // безглуздо.
                if chr.is_whitespace() {
                    continue;
                }
                assert_ne!(
                    *raster, tofu,
                    "{family:?}: у {label} символ {chr:?} малюється порожнім \
                     прямокутником — шрифт його не несе"
                );
                checked += 1;
            }
        }

        // Перевірка, що перевірка щось перевірила: порожня таблиця пройшла б
        // цикл вище мовчки.
        assert!(
            checked > 500,
            "{family:?}: перевірено лише {checked} гліфів — таблиця схудла чи \
             розкладка нічого не повернула"
        );
        println!("  {family:?}: перевірено гліфів: {checked}");
    }
}

/// Моноширинний шрифт лишається моноширинним і на кирилиці.
///
/// Тест вище цього не доводить, і саме тому цей існує: egui, не знайшовши
/// гліфа в `Hack`, мовчки бере його з пропорційного — растр буде справжній,
/// перевірка на тофу зелена, а літера прийде **іншої ширини**. Для приладової
/// панелі це означає стовпчик, що роз'їжджається рівно на українських
/// підписах, тобто помилку, яку видно лише в одній із двох мов.
///
/// Знати це треба до U7c, а не всередині нього.
#[test]
fn the_monospace_family_is_monospace_in_cyrillic_too() {
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(300.0, 300.0),
        )),
        ..Default::default()
    };

    // Латиниця, цифри й кирилиця в одному рядку: усі ширини мають збігтися.
    let text = "iWm019аійЩ";
    let mut widths: Vec<(char, f32)> = Vec::new();
    let mut output = context.run_ui(input, |ui| {
        let galley = ui.painter().layout_no_wrap(
            text.to_string(),
            egui::FontId::monospace(14.0),
            egui::Color32::WHITE,
        );
        widths = galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .map(|glyph| (glyph.chr, glyph.advance_width))
            .collect();
    });
    output.textures_delta.clear();

    let (first_chr, first) = widths[0];
    for (chr, width) in &widths {
        assert!(
            (width - first).abs() < 0.01,
            "у моноширинному {chr:?} має ширину {width}, а {first_chr:?} — {first}: \
             egui підставив гліф з іншого сімейства, і стовпчик роз'їдеться \
             саме на цій літері"
        );
    }
    println!(
        "  моноширинна ширина: {first} на всі {} символів",
        widths.len()
    );
}

/// І окремо — кирилиця, названа поіменно.
///
/// Тест вище впав би й від зниклої латиниці, тобто його червона дошка не
/// каже, що саме сталося. Цей звужує: український алфавіт цілком, включно з
/// ґ, є, і, ї, які випадають із «російського» набору й тому губляться в
/// шрифтах, зібраних не для нас.
#[test]
fn the_ukrainian_alphabet_is_covered_letter_by_letter() {
    let alphabet = "абвгґдеєжзиіїйклмнопрстуфхцчшщьюя\
                    АБВГҐДЕЄЖЗИІЇЙКЛМНОПРСТУФХЦЧШЩЬЮЯ";
    // Δ з «Δv плану», × з «warp ×1000», — з застережень: не кирилиця й не
    // латиниця, тобто губляться окремо від обох.
    let symbols = "Δ×—";

    for family in families() {
        let tofu = one(family.clone(), &MISSING[0].to_string()).1;

        for (glyphs, what) in rasters(family.clone(), &[alphabet.to_string(), symbols.to_string()])
            .iter()
            .zip(["літери", "символу"])
        {
            for (chr, raster) in glyphs {
                assert_ne!(*raster, tofu, "{family:?}: {what} {chr:?} у шрифті немає");
            }
        }
    }
}
