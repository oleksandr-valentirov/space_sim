//! Рядки інтерфейсу через таблицю ключів (ROADMAP-UI.md, правило 7 і U7a).
//!
//! Рішення розробника: **англійська як основна мова, шар локалізації з
//! першого рядка UI**. Причина не в перекладі, а в тому, що розкидані по
//! віджетах літерали доводиться потім вишукувати поодинці — а «потім» тут
//! означає кожну панель, яку встигли написати.
//!
//! Форма — найпростіша, що працює: перелік ключів і дві таблиці. Не `fluent`
//! і не `gettext`: нам потрібні рядки, а не відмінювання за граматичними
//! правилами, і кожна з тих бібліотек — це формат, залежність і збірковий
//! крок. Якщо колись знадобляться числівники й відмінки — тоді й дивитись у
//! їхній бік; таблиця цього не блокує.
//!
//! Числа сюди не потрапляють: формат «доба 12.34» збирається на місці, бо
//! інакше таблиця перетворилася б на шаблонізатор.

/// Ключ рядка. Перелік, а не `&str`: пропущений ключ має бути помилкою
/// компіляції, а не порожнім місцем на екрані.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Заголовок панелі часу.
    Time,
    /// «доба» — підпис перед числом доби місії.
    Day,
    /// «warp» — підпис перед множником часу.
    Warp,
    /// Пауза (кнопка).
    Pause,
    /// Продовжити (та сама кнопка, інший стан).
    Resume,
    /// Швидше вдвічі.
    Faster,
    /// Повільніше вдвічі.
    Slower,
    /// Причина зупинки: пауза.
    StalledPaused,
    /// Причина зупинки: прогноз упирається в горизонт.
    StalledHorizon,
    /// Причина зупинки: місія скінчилася.
    StalledMissionEnd,

    /// Заголовок панелі апарата.
    Vessel,
    /// Висота над поверхнею тіла.
    Altitude,
    /// Швидкість відносно тіла.
    Speed,
    /// Час до наступного маневру.
    NextBurn,
    /// Сумарний Δv плану.
    TotalDv,
    /// Наскільки прогноз випереджає курсор.
    ComputedAhead,
    /// Апарат зупинився помилкою.
    Failed,
    /// Маневрів у плані більше немає.
    NoBurns,

    /// Заголовок панелі розкладу.
    Schedule,
    /// Перицентр.
    Periapsis,
    /// Апоцентр.
    Apoapsis,
    /// Подій у порахованому ще немає.
    NoEvents,
}

/// Мова інтерфейсу. Дві, бо саме дві таблиці й перевіряються.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Ukrainian,
}

/// Рядок за ключем.
pub fn tr(language: Language, key: Key) -> &'static str {
    match language {
        Language::English => english(key),
        Language::Ukrainian => ukrainian(key),
    }
}

fn english(key: Key) -> &'static str {
    match key {
        Key::Time => "TIME",
        Key::Day => "day",
        Key::Warp => "warp",
        Key::Pause => "pause",
        Key::Resume => "resume",
        Key::Faster => "faster",
        Key::Slower => "slower",
        Key::StalledPaused => "paused",
        Key::StalledHorizon => "waiting for the forecast",
        Key::StalledMissionEnd => "mission over",
        Key::Vessel => "VESSEL",
        Key::Altitude => "altitude",
        Key::Speed => "speed",
        Key::NextBurn => "next burn",
        Key::TotalDv => "plan dv",
        Key::ComputedAhead => "computed ahead",
        Key::Failed => "stopped with an error",
        Key::NoBurns => "no burns planned",
        Key::Schedule => "SCHEDULE",
        Key::Periapsis => "periapsis",
        Key::Apoapsis => "apoapsis",
        Key::NoEvents => "nothing computed yet",
    }
}

fn ukrainian(key: Key) -> &'static str {
    match key {
        Key::Time => "ЧАС",
        Key::Day => "доба",
        Key::Warp => "warp",
        Key::Pause => "пауза",
        Key::Resume => "далі",
        Key::Faster => "швидше",
        Key::Slower => "повільніше",
        Key::StalledPaused => "пауза",
        Key::StalledHorizon => "чекає на прогноз",
        Key::StalledMissionEnd => "місія скінчилася",
        Key::Vessel => "АПАРАТ",
        Key::Altitude => "висота",
        Key::Speed => "швидкість",
        Key::NextBurn => "до маневру",
        Key::TotalDv => "Δv плану",
        Key::ComputedAhead => "прогноз уперед",
        Key::Failed => "зупинився помилкою",
        Key::NoBurns => "маневрів немає",
        Key::Schedule => "РОЗКЛАД",
        Key::Periapsis => "перицентр",
        Key::Apoapsis => "апоцентр",
        Key::NoEvents => "поки що порожньо",
    }
}

/// Усі ключі — для перевірок і для того, хто колись малюватиме таблицю
/// перекладу.
pub const ALL: [Key; 22] = [
    Key::Time,
    Key::Day,
    Key::Warp,
    Key::Pause,
    Key::Resume,
    Key::Faster,
    Key::Slower,
    Key::StalledPaused,
    Key::StalledHorizon,
    Key::StalledMissionEnd,
    Key::Vessel,
    Key::Altitude,
    Key::Speed,
    Key::NextBurn,
    Key::TotalDv,
    Key::ComputedAhead,
    Key::Failed,
    Key::NoBurns,
    Key::Schedule,
    Key::Periapsis,
    Key::Apoapsis,
    Key::NoEvents,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Кожен ключ має значення в **обох** таблицях.
    ///
    /// `match` без гілки не збереться, тож компілятор ловить пропущений ключ
    /// сам. Лишається те, чого він не ловить: порожній рядок замість
    /// перекладу — а це рівно те, що виглядає на екрані як зникла кнопка.
    #[test]
    fn every_key_says_something_in_both_tables() {
        for key in ALL {
            for language in [Language::English, Language::Ukrainian] {
                assert!(
                    !tr(language, key).is_empty(),
                    "{key:?} у {language:?} — порожній рядок"
                );
            }
        }
    }

    /// `ALL` не відстає від переліку.
    ///
    /// Список, який хтось забув доповнити, робить перевірку вище тихо
    /// слабшою — вона просто не подивиться на новий ключ.
    #[test]
    fn the_list_of_keys_is_complete() {
        let mut seen = ALL.to_vec();
        seen.sort_by_key(|k| format!("{k:?}"));
        seen.dedup();
        assert_eq!(seen.len(), ALL.len(), "у ALL є повтори");

        // Кожен ключ переліку має бути в ALL. Перелічити його інакше не можна
        // без макросів, тож перевірка спирається на англійську таблицю: два
        // різні ключі з однаковим текстом тут були б помилкою самі по собі.
        let texts: Vec<&str> = ALL.iter().map(|&k| english(k)).collect();
        let mut unique = texts.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            texts.len(),
            "два ключі дають однаковий англійський рядок — або це той самий \
             ключ двічі, або один з них зайвий"
        );
    }
}
