//! Interface strings through a key table (ROADMAP-UI.md, rule 7 and U7a).
//!
//! The developer's decision: **English as the primary language, a localisation
//! layer from the first line of UI**. The reason is not translation but that
//! literals scattered through widgets have to be hunted down one by one
//! afterwards -- and "afterwards" here means every panel written by then.
//!
//! The form is the simplest that works: an enum of keys and two tables. Not
//! `fluent` and not `gettext`: we need strings rather than inflection by
//! grammatical rules, and each of those libraries is a format, a dependency
//! and a build step. If numerals and cases are ever needed, that is when to
//! look their way; the table does not block it.
//!
//! Numbers do not reach here: a format like "day 12.34" is assembled on the
//! spot, because otherwise the table would become a template engine.

/// A string key. An enum rather than a `&str`: a missing key must be a
/// compile error rather than an empty space on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// The time panel's heading.
    Time,
    /// "day" -- the label before the mission day number.
    Day,
    /// "warp" -- the label before the time multiplier.
    Warp,
    /// Pause (a button).
    Pause,
    /// Resume (the same button, a different state).
    Resume,
    /// Twice as fast.
    Faster,
    /// Twice as slow.
    Slower,
    /// Reason for stalling: paused.
    StalledPaused,
    /// Reason for stalling: the prediction is hitting the horizon.
    StalledHorizon,
    /// Reason for stalling: the mission is over.
    StalledMissionEnd,

    /// The vessel panel's heading.
    Vessel,
    /// Altitude above the body's surface.
    Altitude,
    /// Speed relative to the body.
    Speed,
    /// Time to the next manoeuvre.
    NextBurn,
    /// The plan's total dv.
    TotalDv,
    /// How far the prediction runs ahead of the cursor.
    ComputedAhead,
    /// The vessel stopped with an error.
    Failed,
    /// There are no manoeuvres left in the plan.
    NoBurns,

    /// The schedule panel's heading.
    Schedule,
    /// Periapsis.
    Periapsis,
    /// Apoapsis.
    Apoapsis,
    /// There are no events in what is computed yet.
    NoEvents,

    /// The plan panel's heading.
    Plan,
    /// Add a manoeuvre.
    AddBurn,
    /// Fly the plan as shown.
    Commit,
    /// The plan is empty.
    NoPlan,
    /// The plan was rejected: a manoeuvre in the past.
    RejectedInThePast,
    /// The plan was accepted.
    PlanAccepted,

    /// The transfer-window panel's heading.
    Porkchop,
    /// Compute the window grid.
    ComputeWindows,
    /// There is no grid yet.
    NoGrid,
    /// The departure instant.
    Depart,
    /// The flight time.
    FlightTime,
    /// "days" -- the unit after a number.
    Days,
    /// Hyperbolic excess speed at both ends.
    Vinf,
    /// Lambert did not converge in this cell.
    NoSolution,
    /// A hint: hover or click to see a window.
    PickWindow,
    /// The prediction has not left the cursor yet -- there is nothing to build
    /// a grid on.
    NoGridYet,

    /// The view panel's heading.
    View,
    /// The frame the scene is shown in.
    Frame,
    /// The inertial frame (a toggle button).
    FrameInertial,
    /// The Earth-Moon rotating frame (a toggle button).
    FrameRotating,
    /// The zero-velocity curve's label.
    ZeroVelocity,
    /// The main caveat about the curve: it is reference, not a boundary.
    CurveIsAdvice,
    /// The vessel has left the pair -- the curve means nothing there.
    CurveFarAway,

    /// The language switch's label.
    Language,
    /// The name of English -- **in its own language, in both tables**.
    LanguageEnglish,
    /// The name of Ukrainian, likewise.
    LanguageUkrainian,
}

/// The interface language. Two, because exactly two tables are checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Ukrainian,
}

impl Language {
    /// The next language in the cycle, for the switch -- the same pattern as
    /// `ViewFrame`: there are exactly two languages, and a pair of buttons
    /// would imply a "neither selected" state that does not exist.
    pub fn next(self) -> Language {
        match self {
            Language::English => Language::Ukrainian,
            Language::Ukrainian => Language::English,
        }
    }

    /// The key holding a language's own name, so the button is readable to
    /// someone who does not know the current language.
    pub fn name_key(self) -> Key {
        match self {
            Language::English => Key::LanguageEnglish,
            Language::Ukrainian => Key::LanguageUkrainian,
        }
    }
}

/// A string by key.
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
        Key::Plan => "PLAN",
        Key::AddBurn => "add burn",
        Key::Commit => "fly it",
        Key::NoPlan => "no burns yet",
        Key::RejectedInThePast => "refused: that moment has already been flown",
        Key::PlanAccepted => "plan accepted",
        Key::Porkchop => "WINDOWS",
        Key::ComputeWindows => "sweep windows",
        Key::NoGrid => "no grid yet",
        Key::Depart => "depart",
        Key::FlightTime => "flight",
        Key::Days => "days",
        Key::Vinf => "v-inf out / in",
        Key::NoSolution => "no transfer here",
        Key::PickWindow => "point at the plot",
        Key::NoGridYet => "the forecast has not run far enough yet",
        Key::View => "VIEW",
        Key::Frame => "frame",
        Key::FrameInertial => "inertial",
        Key::FrameRotating => "earth-moon rotating",
        Key::ZeroVelocity => "zero-velocity curve, C",
        Key::CurveIsAdvice => "a guide, not a wall: C only holds in the CR3BP",
        Key::CurveFarAway => "the vessel has left the pair - the curve means little there",
        Key::Language => "language",
        // Language names are endonyms and therefore identical in both tables.
        // A button offering "Ukrainian" in English is readable only to someone
        // who already reads English -- exactly the person who does not need
        // it.
        //
        // A side effect more important than the button itself: **Cyrillic is
        // needed even in the English interface**, because this string is in
        // the English table. U7b checks the glyphs for reasons other than the
        // Ukrainian translation.
        Key::LanguageEnglish => "English",
        Key::LanguageUkrainian => "Українська",
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
        Key::Plan => "ПЛАН",
        Key::AddBurn => "додати маневр",
        Key::Commit => "летіти цим",
        Key::NoPlan => "маневрів ще немає",
        Key::RejectedInThePast => "відхилено: ту мить уже пролетіли",
        Key::PlanAccepted => "план прийнято",
        Key::Porkchop => "ВІКНА",
        Key::ComputeWindows => "порахувати вікна",
        Key::NoGrid => "сітки ще немає",
        Key::Depart => "відхід",
        Key::FlightTime => "переліт",
        Key::Days => "діб",
        Key::Vinf => "v-inf туди / там",
        Key::NoSolution => "тут перельоту немає",
        Key::PickWindow => "наведіть на плот",
        Key::NoGridYet => "прогноз ще не відійшов достатньо далеко",
        Key::View => "ВИГЛЯД",
        Key::Frame => "фрейм",
        Key::FrameInertial => "інерціальний",
        Key::FrameRotating => "обертовий Земля-Місяць",
        Key::ZeroVelocity => "крива нульової швидкості, C",
        Key::CurveIsAdvice => "довідка, а не межа: C зберігається лише в CR3BP",
        Key::CurveFarAway => "апарат пішов від пари — там крива майже ні про що",
        Key::Language => "мова",
        Key::LanguageEnglish => "English",
        Key::LanguageUkrainian => "Українська",
    }
}

/// Every key -- for the checks, and for whoever one day draws a translation
/// table.
pub const ALL: [Key; 48] = [
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
    Key::Plan,
    Key::AddBurn,
    Key::Commit,
    Key::NoPlan,
    Key::RejectedInThePast,
    Key::PlanAccepted,
    Key::Porkchop,
    Key::ComputeWindows,
    Key::NoGrid,
    Key::Depart,
    Key::FlightTime,
    Key::Days,
    Key::Vinf,
    Key::NoSolution,
    Key::PickWindow,
    Key::NoGridYet,
    Key::View,
    Key::Frame,
    Key::FrameInertial,
    Key::FrameRotating,
    Key::ZeroVelocity,
    Key::CurveIsAdvice,
    Key::CurveFarAway,
    Key::Language,
    Key::LanguageEnglish,
    Key::LanguageUkrainian,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key has a value in **both** tables.
    ///
    /// A `match` without an arm does not compile, so the compiler catches a
    /// missing key itself. What remains is what it does not catch: an empty
    /// string instead of a translation -- which is exactly what looks on
    /// screen like a vanished button.
    #[test]
    fn every_key_says_something_in_both_tables() {
        for key in ALL {
            for language in [Language::English, Language::Ukrainian] {
                assert!(
                    !tr(language, key).is_empty(),
                    "{key:?} in {language:?} is an empty string"
                );
            }
        }
    }

    /// The language names are identical in both tables -- deliberately.
    ///
    /// An endonym stays itself in any interface: translating "Українська" into
    /// "Ukrainian" would make the button unreadable to exactly the person
    /// looking for it. The check is here because the rule is easy to break
    /// with the best of intentions.
    #[test]
    fn the_names_of_languages_are_the_same_in_both_tables() {
        for key in [Key::LanguageEnglish, Key::LanguageUkrainian] {
            assert_eq!(
                tr(Language::English, key),
                tr(Language::Ukrainian, key),
                "{key:?} was translated, but should have stayed an endonym"
            );
        }
    }

    /// The switch cycles and returns to itself.
    #[test]
    fn the_switch_comes_back_to_where_it_started() {
        for language in [Language::English, Language::Ukrainian] {
            assert_ne!(language.next(), language, "the switch stands still");
            assert_eq!(language.next().next(), language);
            // The button is labelled with the name of the language it will
            // switch to.
            assert!(!tr(language, language.next().name_key()).is_empty());
        }
    }

    /// `ALL` does not fall behind the enum.
    ///
    /// A list somebody forgot to extend makes the check above quietly
    /// weaker -- it simply will not look at the new key.
    #[test]
    fn the_list_of_keys_is_complete() {
        let mut seen = ALL.to_vec();
        seen.sort_by_key(|k| format!("{k:?}"));
        seen.dedup();
        assert_eq!(seen.len(), ALL.len(), "у ALL є повтори");

        // Every key of the enum must be in ALL. It cannot be enumerated any
        // other way without macros, so the check leans on the English table:
        // two different keys with identical text would be an error in
        // themselves.
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
