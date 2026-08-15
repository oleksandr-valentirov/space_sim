//! Приладова палітра (ROADMAP-UI.md, U7c).
//!
//! PROJECT.md §2 називає естетику центру керування польотами **стилем, а не
//! компромісом**. Отже це не «зробити гарніше», а рішення про те, що кольори
//! в цій грі **означають**, і воно одне на сцену й на інтерфейс.
//!
//! ## Головне рішення: палітра виводиться з траєкторій, а не навпаки
//!
//! Кольори ліній існували до цього кроку (`view.rs`, H5), і вони вже несли
//! зміст: помаранчевий — прогноз, приглушений синій — історія, зелений —
//! непідтверджене прев'ю, білий — сам апарат. Інтерфейс, пофарбований окремо,
//! почав би говорити **другою** мовою кольору поверх першої: кнопка «летіти
//! цим» синя, а прев'ю, яке вона підтверджує, зелене.
//!
//! Тому акценти інтерфейсу — це ті самі чотири кольори, і жодного нового:
//!
//! | колір | у сцені | в інтерфейсі |
//! |---|---|---|
//! | бурштин | прогноз, майбутнє | активне, те, що коштує |
//! | синій | історія, минуле | довідка, вимкнене |
//! | зелений | прев'ю плану | дія, яку ще не підтвердили |
//! | білий | апарат | те, на чому фокус |
//!
//! Перевіряється це числом, а не оком: [`ACCENT`] зобов'язаний дорівнювати
//! кольору прогнозу, і тест валиться, якщо їх розвести.
//!
//! ## Один колірний простір, без перетворень
//!
//! Ціль кадру — `Rgba8Unorm` **без sRGB** (`engine::shot::FORMAT`), тобто
//! байти проходять у знімок такими, якими їх поклали, а PNG потім читається
//! як sRGB. Отже «лінійна ціль» тут означає «без апаратного перетворення», а
//! не «в лінійному світлі»: числа, які вже стоять у `view.rs`, підбирались по
//! тому, як вони виглядають, тобто фактично в sRGB.
//!
//! Тому палітра тримає **вісім біт на канал** і роздає їх обом шляхам без
//! гамми: [`Colour::scene`] ділить на 255, [`Colour::egui`] віддає ті самі
//! байти. Перетворення, зроблене «правильно» на одному з двох шляхів, розвело
//! б однаковий колір на два різні — і саме це U7c2 міряє знімком.

use engine::egui;

/// Колір палітри: вісім біт на канал, sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour(pub u8, pub u8, pub u8);

impl Colour {
    /// Для сцени: `[f32; 4]` у тому вигляді, якого хоче `Polyline`.
    pub fn scene(self) -> [f32; 4] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
            1.0,
        ]
    }

    /// Для інтерфейсу: ті самі байти, без перетворення.
    pub fn egui(self) -> egui::Color32 {
        egui::Color32::from_rgb(self.0, self.1, self.2)
    }

    /// Той самий колір, притлумлений до частки `k` від повної яскравості.
    ///
    /// Потрібен там, де відтінок мусить лишитись тим самим, а вага — впасти:
    /// рамка проти заливки, вимкнена кнопка проти живої. Окремий колір у
    /// таблиці для цього завів би другий бурштин, який згодом розійшовся б з
    /// першим.
    pub fn dim(self, k: f32) -> Colour {
        let scale = |c: u8| (c as f32 * k).round().clamp(0.0, 255.0) as u8;
        Colour(scale(self.0), scale(self.1), scale(self.2))
    }
}

// ---------------------------------------------------------------------------
// Чотири кольори, що несуть зміст. Решта палітри — тло для них.

/// Прогноз: те, що попереду. Він же акцент інтерфейсу — [`ACCENT`].
pub const PREDICTION: Colour = Colour(229, 153, 51);

/// Історія: те, що вже пролетіли. Навмисно тихіший за прогноз — минуле не
/// має перетягувати око з того місця, де рухається межа.
pub const HISTORY: Colour = Colour(89, 115, 153);

/// Прев'ю плану: пораховане, але не підтверджене.
pub const PREVIEW: Colour = Colour(102, 229, 128);

/// Сам апарат.
pub const VESSEL: Colour = Colour(255, 255, 255);

/// Акцент інтерфейсу — **той самий колір, що прогноз**, і це перевіряється.
pub const ACCENT: Colour = PREDICTION;

/// Дія, яку ще не підтвердили, — той самий колір, що прев'ю.
pub const ACTION: Colour = PREVIEW;

// ---------------------------------------------------------------------------
// Тло. Темніше за небо кадру, щоб панель читалася поверх нього.

/// Небо кадру в тих самих одиницях — `engine::frame::CLEAR_BYTES`.
///
/// Копія тут не для використання, а для перевірки: панель мусить бути
/// **темнішою** за небо, інакше вона світиться на тлі космосу замість лежати
/// на ньому. Тест звіряє це з рушієм, тож розбіжність не проживе.
pub const SKY: Colour = Colour(5, 8, 20);

/// Заливка панелі.
///
/// Темніша за небо по кожному каналу, і це не смак: у кадрі під панеллю буває
/// не тільки космос, а й освітлений диск планети. Панель, розрахована на
/// темне тло, поверх нього перестала б читатися — тож вона щільна й темна, а
/// проти самого неба через це читається як провал. Так і задумано; перевіряє
/// це `the_panel_is_darker_than_the_sky_behind_it`, і перша його редакція
/// впала саме тут.
pub const PANEL: Colour = Colour(4, 6, 12);

/// Поверхня віджета в спокої.
pub const SURFACE: Colour = Colour(24, 31, 43);

/// Те саме під курсором.
pub const SURFACE_HOVER: Colour = Colour(38, 48, 65);

/// І натиснуте.
pub const SURFACE_ACTIVE: Colour = Colour(52, 65, 86);

/// Лінії: рамки, розділювачі.
pub const LINE: Colour = Colour(48, 60, 78);

/// Основний текст.
pub const TEXT: Colour = Colour(201, 211, 223);

/// Другорядний текст: одиниці, підказки, те, що не число.
pub const TEXT_DIM: Colour = Colour(124, 136, 152);

/// Відмова: план відхилено, апарат зупинився помилкою.
///
/// Єдиний колір поза чотирма змістовними, і він заслужив окремий рядок:
/// «щось пішло не так» не можна сказати жодним із них, не збрехавши про
/// зміст. Червоний тут приглушений — панель не має кричати.
pub const ALARM: Colour = Colour(214, 97, 85);

// ---------------------------------------------------------------------------
// Шкала porkchop. Кінці — з тієї ж палітри, і це не оформлення.

/// Дешевий кінець шкали вікон.
///
/// Споріднений з [`HISTORY`], а не новий синій: обидва означають «спокійне,
/// на що не треба дивитись».
pub const CHEAP: Colour = Colour(30, 70, 160);

/// Дорогий кінець — [`PREDICTION`], бо бурштин у цій грі скрізь означає «те,
/// що коштує».
pub const COSTLY: Colour = PREDICTION;

/// Ставить палітру в контекст: тема, стиль, обидві гілки теми.
///
/// Живе тут, а не в `app`, бо той самий виклик потрібен кожному, хто малює
/// інтерфейс без вікна — тестам і знімкам. Панель, знята з типовим стилем,
/// показувала б не те, що бачить гравець, і як оракул була б гіршою за
/// відсутню.
pub fn apply(context: &egui::Context) {
    let style = style();
    context.set_theme(egui::ThemePreference::Dark);
    // Обом темам той самий стиль: світлої теми в цій грі немає, і приладова
    // панель, що побіліла від системної налаштовки, — це зламаний кадр, а не
    // варіант оформлення.
    context.set_style_of(egui::Theme::Dark, style.clone());
    context.set_style_of(egui::Theme::Light, style);
}

/// Стиль egui цілком (ROADMAP-UI.md, U7c).
///
/// ## Чому все моноширинне
///
/// Приладова панель — це стовпчики чисел, які око порівнює зверху вниз.
/// Пропорційний шрифт робить «7» вужчою за «0», тобто зсуває розряди, і
/// висота 412 км стає ширшою за 400 км. U7b виміряв, що моноширинне сімейство
/// egui несе кирилицю **власними гліфами тієї самої ширини**, тож ціна цього
/// рішення — нуль, і воно доступне лише тому, що той крок пройшов першим.
///
/// ## Щільність
///
/// Відступи скупіші за типові egui: панель ліворуч має 220 точок ширини
/// (`app::draw`), і типові 8 точок між елементами з'їдають екран рядками
/// повітря. Приладова щільність — це не косметика, а кількість рядків, які
/// видно одночасно.
pub fn style() -> egui::Style {
    let mut style = egui::Style {
        visuals: visuals(),
        ..Default::default()
    };

    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.indent = 12.0;
    style.spacing.interact_size.y = 18.0;

    // Усе моноширинне, крім заголовка панелі: він і так один рядок, а трохи
    // більший кегль відділяє його від чисел без жодної лінії.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Monospace)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Monospace)),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ]
    .into();

    style
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = PANEL.egui();
    visuals.window_fill = PANEL.egui();
    visuals.extreme_bg_color = SKY.egui();
    visuals.override_text_color = Some(TEXT.egui());
    visuals.window_stroke = egui::Stroke::new(1.0, LINE.egui());

    // Виділене — бурштином, тобто тим самим, чим прогноз. Текст на ньому
    // темний: бурштин світлий, і світлий текст на ньому не читається.
    visuals.selection.bg_fill = ACCENT.dim(0.55).egui();
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT.egui());

    let widget = |bg: Colour, stroke: Colour, text: Colour| egui::style::WidgetVisuals {
        bg_fill: bg.egui(),
        weak_bg_fill: bg.egui(),
        bg_stroke: egui::Stroke::new(1.0, stroke.egui()),
        fg_stroke: egui::Stroke::new(1.0, text.egui()),
        corner_radius: egui::CornerRadius::ZERO,
        expansion: 0.0,
    };

    // Прямі кути навмисно: заокруглення — мова м'якого інтерфейсу, а тут
    // приладова панель. Це рівно те місце, де «стиль, а не компроміс» видно
    // одним полем.
    visuals.widgets.noninteractive = widget(PANEL, LINE.dim(0.6), TEXT_DIM);
    visuals.widgets.inactive = widget(SURFACE, LINE, TEXT);
    visuals.widgets.hovered = widget(SURFACE_HOVER, ACCENT.dim(0.7), TEXT);
    visuals.widgets.active = widget(SURFACE_ACTIVE, ACCENT, ACCENT);
    visuals.widgets.open = widget(SURFACE_HOVER, LINE, TEXT);

    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Акцент інтерфейсу — це колір прогнозу, а не схожий на нього.
    ///
    /// Уся ідея палітри тримається на цьому рядку: інтерфейс і сцена говорять
    /// однією мовою кольору. Два «майже однакові» бурштини — це та сама
    /// помилка, тільки непомітна, тож перевіряється рівність, а не близькість.
    #[test]
    fn the_accent_is_the_colour_of_the_forecast() {
        assert_eq!(ACCENT, PREDICTION);
        assert_eq!(ACTION, PREVIEW);
        assert_eq!(COSTLY, PREDICTION);
    }

    /// Панель темніша за небо кадру.
    ///
    /// Інакше вона світиться на тлі космосу замість лежати на ньому — і це та
    /// помилка, яку на чорному моніторі не видно, а на яскравому видно одразу.
    /// Небо береться з рушія, а не з копії поруч: копія розійшлася б мовчки.
    #[test]
    fn the_panel_is_darker_than_the_sky_behind_it() {
        assert_eq!(
            [SKY.0, SKY.1, SKY.2],
            engine::frame::CLEAR_BYTES,
            "копія кольору неба розійшлася з рушієм"
        );

        let weight = |c: Colour| c.0 as u32 + c.1 as u32 + c.2 as u32;
        assert!(
            weight(PANEL) < weight(SKY),
            "панель {PANEL:?} не темніша за небо {SKY:?}"
        );
    }

    /// Текст читається на тому, на чому лежить.
    ///
    /// Контраст — це формула, а не смак: WCAG рахує відношення відносних
    /// яскравостей, і 4.5:1 — межа для основного тексту. Перевірка тут саме
    /// тому, що «мені видно» на одному моніторі нічого не доводить про інший.
    #[test]
    fn the_text_has_enough_contrast_against_its_background() {
        // Відносна яскравість sRGB за означенням WCAG.
        fn luminance(c: Colour) -> f64 {
            let channel = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
        }

        fn contrast(a: Colour, b: Colour) -> f64 {
            let (x, y) = (luminance(a), luminance(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            (hi + 0.05) / (lo + 0.05)
        }

        for (text, background, floor, what) in [
            (TEXT, PANEL, 4.5, "основний текст на панелі"),
            (TEXT, SURFACE, 4.5, "основний текст на кнопці"),
            (TEXT, SURFACE_HOVER, 4.5, "текст на кнопці під курсором"),
            // Другорядний — це підказки й одиниці, для них WCAG дозволяє 3:1
            // як для великого тексту; нижче цього він перестає бути текстом.
            (TEXT_DIM, PANEL, 3.0, "другорядний текст"),
            (ACCENT, PANEL, 4.5, "акцент на панелі"),
            (ALARM, PANEL, 4.5, "відмова на панелі"),
            (PREVIEW, PANEL, 4.5, "дія на панелі"),
        ] {
            let ratio = contrast(text, background);
            assert!(
                ratio >= floor,
                "{what}: контраст {ratio:.2} проти потрібних {floor} \
                 ({text:?} на {background:?})"
            );
        }
    }

    /// [`apply`] справді доходить до контексту — і міняє те, що було.
    ///
    /// Друга половина не дає тесту бути тавтологією: перевірити, що
    /// `style().visuals.panel_fill == PANEL`, означало б перевірити знак
    /// присвоєння. Значення має те, що типовий egui дає **інше**, тобто
    /// палітра щось насправді робить.
    #[test]
    fn the_style_reaches_the_context_and_differs_from_the_default() {
        let plain = egui::Context::default();
        let styled = egui::Context::default();
        apply(&styled);

        let fill = |context: &egui::Context| context.style_of(egui::Theme::Dark).visuals.panel_fill;
        assert_eq!(fill(&styled), PANEL.egui());
        assert_ne!(
            fill(&plain),
            fill(&styled),
            "палітра дала те саме, що типовий egui — тобто не дала нічого"
        );

        // Обом темам, а не лише активній: світлої теми в цій грі немає.
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            assert_eq!(
                styled.style_of(theme).visuals.panel_fill,
                PANEL.egui(),
                "{theme:?} лишилась із типовим стилем"
            );
        }
    }

    /// Панель набрана моноширинним — усе, крім нічого.
    ///
    /// Рішення легко втратити при першому ж «поправлю кегль», а коштує воно
    /// стовпчиків чисел, які перестають шикуватися. U7b довів, що кирилиця в
    /// цьому сімействі своя й тієї самої ширини, тож ціна рішення — нуль.
    #[test]
    fn every_text_style_is_monospace() {
        let style = style();
        for (which, font) in &style.text_styles {
            assert_eq!(
                font.family,
                egui::FontFamily::Monospace,
                "{which:?} набраний не моноширинним — стовпчик чисел роз'їдеться"
            );
        }
        assert!(
            style.text_styles.len() >= 5,
            "стилі тексту зникли з таблиці"
        );
    }

    /// Притлумлення не міняє відтінку — воно міняє вагу.
    #[test]
    fn dimming_keeps_the_hue() {
        let dim = ACCENT.dim(0.5);
        assert!(dim.0 < ACCENT.0 && dim.1 < ACCENT.1 && dim.2 < ACCENT.2);
        // Відношення каналів збережене з точністю до округлення байта.
        let ratio = |c: Colour| c.0 as f32 / c.1 as f32;
        assert!((ratio(dim) - ratio(ACCENT)).abs() < 0.05);
    }
}
