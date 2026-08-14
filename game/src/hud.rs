//! Панелі, що показують (ROADMAP-UI.md, U2).
//!
//! ## Дві межі, які тут тримаються
//!
//! **Панель нічого не рахує наперед і нічого не пам'ятає** (правило 1): усе,
//! що вона показує, виводиться зі снапшоту в цьому ж кадрі. Тому функції тут
//! беруть снапшот і повертають **команди**, а не надсилають їх: хто надсилає,
//! той і знає про канал, а панель має знати лише про те, що намальовано.
//!
//! **Панель не кличе ефемериду й не пропагує** (правило 5). Календарна дата —
//! єдине обчислення тут, і воно з арифметики, а не з ассета.
//!
//! Наслідок для перевірки: панель малюється в тесті без вікна, а «клік по
//! паузі кладе рівно `TogglePause` і **нічого більше**» — це порівняння
//! повернутого вектора, а не спостереження за грою.

use engine::egui;

use crate::clock::Stall;
use crate::mission;
use crate::sim::Command;
use crate::snapshot::WorldSnapshot;
use crate::text::{tr, Key, Language};

/// Секунд у добі. Тут — не фізична стала, а одиниця показу.
const DAY_S: f64 = 86400.0;

/// Зсув шкали ассета (TT) до UT1, секунди (ROADMAP K3b).
///
/// Стала, і це записано чесно: реальний TT−UT1 непередбачуваний, бо залежить
/// від обертання Землі, яке ніхто не гарантує наперед. Отже дата на екрані
/// точна до секунди-двох на століття — і саме тому вона властивість UI, а не
/// фізики (у зворотний бік це число не повертається ніколи).
const TT_MINUS_UT1_S: f64 = 63.8286;

/// Панель часу: доба місії, дата, warp, причина зупинки, кнопки.
///
/// Повертає команди в порядку натискання. Порожній вектор — нормальний
/// результат: гравець просто дивиться.
pub fn time_panel(ui: &mut egui::Ui, language: Language, snapshot: &WorldSnapshot) -> Vec<Command> {
    let mut commands = Vec::new();

    let day = (snapshot.t - mission::start().t) / DAY_S;
    // Пауза читається зі `stall`, а не з warp: `Clock::warp()` віддає
    // **заданий** множник і в паузі не обнуляється — інакше натиснути «далі»
    // означало б згадати, на чому зупинились.
    let paused = snapshot.stall == Some(Stall::Paused);

    ui.heading(tr(language, Key::Time));
    ui.label(format!(
        "{} {day:.2} / {:.2}",
        tr(language, Key::Day),
        mission::DAYS
    ));
    ui.label(calendar(snapshot.t));
    ui.label(format!("{} ×{:.0}", tr(language, Key::Warp), snapshot.warp));

    // Причина зупинки — словами. Мовчазне підгальмовування виглядає як
    // зламана гра, а не як «прогноз ще рахується».
    if let Some(stall) = snapshot.stall {
        ui.label(tr(
            language,
            match stall {
                Stall::Paused => Key::StalledPaused,
                Stall::Horizon => Key::StalledHorizon,
                Stall::MissionEnd => Key::StalledMissionEnd,
            },
        ));
    }

    ui.horizontal(|ui| {
        let pause = if paused { Key::Resume } else { Key::Pause };
        if button(ui, PAUSE, tr(language, pause)) {
            commands.push(Command::TogglePause);
        }
        if button(ui, SLOWER, tr(language, Key::Slower)) {
            commands.push(Command::ScaleWarp(0.5));
        }
        if button(ui, FASTER, tr(language, Key::Faster)) {
            commands.push(Command::ScaleWarp(2.0));
        }
    });

    commands
}

/// Сталі адреси кнопок панелі часу.
///
/// Потрібні не грі, а перевірці: без них тест шукав би кнопку підібраними
/// пікселями, і від першої зміни відступів почав би клікати в порожнечу,
/// лишаючись зеленим. Підпис для цього не годиться — він змінюється разом
/// із мовою.
pub const PAUSE: &str = "hud.time.pause";
pub const SLOWER: &str = "hud.time.slower";
pub const FASTER: &str = "hud.time.faster";

/// Кнопка зі сталою адресою.
///
/// egui дає віджетам автоматичні `Id`, відтворити які ззовні не можна, тож
/// поверх намальованої кнопки заводиться друга взаємодія — з нашим іменем і
/// тим самим прямокутником. Вона й вирішує, чи був клік: зареєстрована
/// пізніше, тобто лежить зверху.
fn button(ui: &mut egui::Ui, id: &str, label: &str) -> bool {
    let drawn = ui.button(label);
    ui.interact(drawn.rect, egui::Id::new(id), egui::Sense::click())
        .clicked()
}

/// Календарна дата з секунд від епохи ассета (J2000 TDB), UTC-подібна.
///
/// TDB замість TT нічого тут не змінює: різниця між ними періодична й не
/// перевищує 1.7 мс, тобто лежить на три порядки нижче за секунду, якою
/// закінчується цей рядок. Записано, щоб наступний читач не шукав похибку
/// там, де її немає.
///
/// Перетворення живе в грі, а не у фізиці: воно кличе рівно те, чого в циклі
/// інтегрування бути не може, і назад у нього не повертається (ROADMAP-UI.md,
/// U2b). Алгоритм — цивільний календар із номера дня (Хаувард Гіннант,
/// `civil_from_days`), цілочисельний і без жодної тригонометрії.
pub fn calendar(t: f64) -> String {
    // J2000 — це 2000-01-01 12:00:00, тобто полудень. Півдоби зсуву роблять
    // з нього північ, від якої рахуються дні.
    let seconds = t - TT_MINUS_UT1_S + 0.5 * DAY_S;
    let days = seconds.div_euclid(DAY_S);
    let rest = seconds.rem_euclid(DAY_S);

    // Днів від 1970-01-01 до 2000-01-01 — 10957.
    let (year, month, day) = civil_from_days(days as i64 + 10957);

    let hour = (rest / 3600.0) as u32;
    let minute = ((rest % 3600.0) / 60.0) as u32;
    let second = (rest % 60.0) as u32;

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// День від 1970-01-01 → (рік, місяць, день). Цілочисельний, без бібліотек.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Епоха ассета — це J2000, тобто полудень першого січня 2000-го.
    ///
    /// Оракул тут — означення епохи, а не інша реалізація того самого
    /// перетворення: друга реалізація помилялася б разом із першою, якби я
    /// переплутав напрямок зсуву.
    #[test]
    fn the_epoch_is_noon_on_the_first_of_january_2000() {
        let text = calendar(0.0);
        assert!(
            text.starts_with("2000-01-01 11:58:5"),
            "епоха дала {text}, а мала дати полудень мінус 63.8 с (TT−UT1)"
        );
    }

    /// Доба вперед — це наступний день у ту саму хвилину.
    #[test]
    fn a_day_later_is_the_next_day() {
        assert!(calendar(DAY_S).starts_with("2000-01-02 11:58:5"));
    }

    /// І високосний рік проходиться наскрізь, а не обходиться.
    ///
    /// 2000-й — високосний (ділиться на 400), і це той випадок, який валить
    /// наївне «кожні чотири роки, крім сотих».
    #[test]
    fn the_year_2000_has_a_twenty_ninth_of_february() {
        // 59 діб від 1 січня (полудень) — це 29 лютого.
        let text = calendar(59.0 * DAY_S);
        assert!(text.starts_with("2000-02-29"), "вийшло {text}");
    }
}
