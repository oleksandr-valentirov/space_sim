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
use crate::plan::{Frame, Manoeuvre, Plan};
use crate::porkchop::{cell_at, colour, Grid};
use crate::schedule::{Kind, Marker};
use crate::sim::Command;
use crate::snapshot::{VesselSnapshot, WorldSnapshot};
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

/// Панель розкладу: маркери подій, клік по рядку — перемотати туди
/// (ROADMAP-UI.md, U3b).
///
/// Показуються лише події **попереду курсора**: минуле вже пролетіли, а назад
/// курсор не ходить (J-етап), тож рядок «перицентр учора» був би кнопкою, яка
/// завжди відмовляє.
pub fn schedule_panel(
    ui: &mut egui::Ui,
    language: Language,
    now: f64,
    markers: &[Marker],
) -> Vec<Command> {
    let mut commands = Vec::new();

    ui.heading(tr(language, Key::Schedule));

    let mut shown = 0;
    for (index, marker) in markers.iter().enumerate() {
        if marker.t <= now {
            continue;
        }

        let name = tr(
            language,
            match marker.kind {
                Kind::Periapsis => Key::Periapsis,
                Kind::Apoapsis => Key::Apoapsis,
            },
        );
        let label = format!(
            "{name}: +{:.2} діб, {:.0} км",
            (marker.t - now) / DAY_S,
            marker.distance_m / 1000.0
        );

        // Адреса рядка — його порядковий номер у списку маркерів, а не час:
        // час — `f64`, і як частина `Id` він перетворив би найменше уточнення
        // інтерполяції на інший віджет.
        if button(ui, &format!("{SEEK}{index}"), &label) {
            commands.push(Command::SeekTo(marker.t));
        }

        shown += 1;
        if shown >= MAX_ROWS {
            break;
        }
    }

    if shown == 0 {
        ui.label(tr(language, Key::NoEvents));
    }

    commands
}

/// Префікс адрес рядків розкладу.
pub const SEEK: &str = "hud.schedule.seek.";

/// Скільки подій показувати. Розклад — це «куди перемотати далі», а не
/// журнал: перші кілька відповідають на це питання, решта лише займає екран.
const MAX_ROWS: usize = 6;

/// Чернетка плану, яку редагує гравець.
///
/// Це той рідкісний власний стан UI, який правило 1 дозволяє: **редагований,
/// але ще не поданий план не існує поза екраном**. Щойно гравець просить
/// прев'ю або коміт, чернетка перетворюється на `Plan` і йде в нитку — і з
/// того моменту істина знову в снапшоті.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanDraft {
    pub manoeuvres: Vec<Manoeuvre>,
}

impl PlanDraft {
    /// Чернетка з плану, яким апарат летить зараз.
    pub fn from_plan(plan: &Plan) -> PlanDraft {
        PlanDraft {
            manoeuvres: plan.manoeuvres().to_vec(),
        }
    }

    /// План у тому вигляді, в якому його прийме світ.
    ///
    /// `Plan::insert` тримає порядок за часом сам, тож чернетка не зобов'язана
    /// його берегти: гравець може посунути маневр у минуле відносно сусіда, і
    /// це не має ставати помилкою редагування.
    pub fn plan(&self) -> Plan {
        let mut plan = Plan::new();
        for manoeuvre in &self.manoeuvres {
            plan.insert(*manoeuvre);
        }
        plan
    }
}

/// Що панель плану просить зробити.
///
/// Обидва варіанти несуть **той самий** план, який показано на екрані, і в
/// цьому вся суть кроку: лінія, яку бачив, і є лінія, якою полетиш (J5).
#[derive(Clone, Debug, PartialEq)]
pub enum PlanAction {
    /// Показати, що вийде. Іде в планувальник, у світ не пише нічого.
    Preview(Plan),
    /// Летіти цим. Іде в нитку симуляції.
    Commit(Plan),
}

/// Адреси віджетів плану. Ті самі міркування, що в `SEEK`: тест мусить
/// знаходити віджет за іменем, а не за пікселем.
pub const PLAN_ADD: &str = "hud.plan.add";
pub const PLAN_COMMIT: &str = "hud.plan.commit";
pub const PLAN_DELETE: &str = "hud.plan.delete.";

/// Панель плану: маневри рядками, час і три компоненти Δv у VNB.
///
/// `draft` — власний стан UI (див. [`PlanDraft`]); `notice` — те, що світ
/// відповів на попередню спробу, і панель показує саме його, а не власне
/// припущення про успіх (правило 8).
pub fn plan_panel(
    ui: &mut egui::Ui,
    language: Language,
    now: f64,
    body: i32,
    draft: &mut PlanDraft,
    notice: Option<&str>,
) -> Vec<PlanAction> {
    let mut actions = Vec::new();
    let mut changed = false;

    ui.heading(tr(language, Key::Plan));

    let mut delete = None;
    for (index, manoeuvre) in draft.manoeuvres.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            // Час у добах від «зараз» — те, чим гравець думає. У план він
            // іде абсолютним, як вимагає `Manoeuvre::t`.
            let mut days = (manoeuvre.t - now) / DAY_S;
            let response = ui.add(
                egui::DragValue::new(&mut days)
                    .speed(0.01)
                    .range(-3650.0..=3650.0),
            );
            if response.changed() {
                manoeuvre.t = now + days * DAY_S;
                changed = true;
            }

            for axis in 0..3 {
                let mut value = manoeuvre.dv[axis];
                let response = ui.add(egui::DragValue::new(&mut value).speed(0.1));
                if response.changed() {
                    manoeuvre.dv[axis] = value;
                    changed = true;
                }
            }

            if button(ui, &format!("{PLAN_DELETE}{index}"), "×") {
                delete = Some(index);
            }
        });
    }

    if let Some(index) = delete {
        draft.manoeuvres.remove(index);
        changed = true;
    }

    ui.horizontal(|ui| {
        if button(ui, PLAN_ADD, tr(language, Key::AddBurn)) {
            draft.manoeuvres.push(Manoeuvre {
                // Доба вперед — не «зараз»: маневр у поточній миті світ
                // відхилить, бо курсор його вже проходить.
                t: now + DAY_S,
                dv: [0.0; 3],
                frame: Frame::Vnb { body },
            });
            changed = true;
        }
        if button(ui, PLAN_COMMIT, tr(language, Key::Commit)) {
            actions.push(PlanAction::Commit(draft.plan()));
        }
    });

    if let Some(text) = notice {
        ui.label(text);
    }

    // Прев'ю — після коміту в списку дій, бо зміна цього кадру могла бути
    // саме тією, яку коміт і забирає.
    if changed {
        actions.push(PlanAction::Preview(draft.plan()));
    }

    actions
}

/// Стан плоту вікон, який існує лише на екрані (ROADMAP-UI.md, U5c).
///
/// Той самий виняток із правила 1, що [`PlanDraft`]: текстура — це сітка,
/// перекладена в пікселі, а обране вікно — це «на що я дивлюсь», і поза
/// екраном ні того, ні того немає. Числа при цьому не запам'ятовуються жодні:
/// усе, що показано, щоразу виводиться з `Grid`.
#[derive(Default)]
pub struct PlotState {
    /// Текстура й номер сітки, з якої вона зроблена.
    ///
    /// Номер тут не для порядку, а щоб не перебудовувати зображення щокадру:
    /// сітка 100×100 — це 10⁴ пікселів, і сама вона не змінюється взагалі,
    /// доки не приїде наступна.
    texture: Option<(u64, egui::TextureHandle)>,
    /// Обране вікно — індекси на осях, а не час: осі задає той, хто просив
    /// сітку, і тримати другу копію їхніх значень означало б дати їм
    /// розійтися.
    pub chosen: Option<(usize, usize)>,
}

/// Що плот просить зробити.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PorkchopAction {
    /// Порахувати сітку — кнопка. Осі вибирає той, хто надсилає запит.
    Compute,
    /// Гравець обрав вікно: індекси на осях сітки.
    Choose(usize, usize),
}

/// Адреси віджетів плоту.
pub const PLOT_COMPUTE: &str = "hud.porkchop.compute";
pub const PLOT_IMAGE: &str = "hud.porkchop.image";

/// Скільки пікселів екрана віддати плоту. Квадрат: осі різні за змістом, але
/// однакові за важливістю, і витягнутий плот читається як «одна з них
/// точніша».
const PLOT_SIDE: f32 = 200.0;

/// Числа обраного (чи наведеного) вікна — те, що показує курсор.
///
/// Окремо від малювання з тієї ж причини, що [`VesselReadout`]: оракул кроку —
/// «число в панелі дорівнює числу в сітці», а пікселі з числами не звіряються.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowReadout {
    /// Момент відходу, абсолютний час ассета.
    pub t1: f64,
    /// Тривалість перельоту, секунди.
    pub tof: f64,
    /// Клітинка; `None` — заборонена зона, і саме так її треба показати.
    pub cell: Option<crate::porkchop::Cell>,
}

/// Числа вікна за індексами на осях. Нічого не рахує, крім вибірки.
pub fn read_window(grid: &Grid, i_t1: usize, i_tof: usize) -> Option<WindowReadout> {
    Some(WindowReadout {
        t1: *grid.t1.get(i_t1)?,
        tof: *grid.tof.get(i_tof)?,
        cell: grid.at(i_t1, i_tof),
    })
}

/// Панель плоту: сітка зображенням, осі в датах, курсор із числами.
///
/// `grid` — те, що порахувала нитка планувальника (правило 6); `None` означає
/// «ще не просили» або «ще рахується», і панель у цьому разі показує кнопку й
/// нічого не вигадує.
pub fn porkchop_panel(
    ui: &mut egui::Ui,
    language: Language,
    grid: Option<&Grid>,
    state: &mut PlotState,
) -> Vec<PorkchopAction> {
    let mut actions = Vec::new();

    ui.heading(tr(language, Key::Porkchop));

    if button(ui, PLOT_COMPUTE, tr(language, Key::ComputeWindows)) {
        actions.push(PorkchopAction::Compute);
    }

    let Some(grid) = grid else {
        ui.label(tr(language, Key::NoGrid));
        return actions;
    };

    // Текстура будується один раз на сітку, а не на кадр.
    let texture = match &state.texture {
        Some((id, texture)) if *id == grid.id => texture.clone(),
        _ => {
            let texture = ui.ctx().load_texture(
                "porkchop",
                image_of(grid),
                // Без згладжування: піксель — це клітинка, і розмита межа
                // між клітинкою й діркою — це вигаданий проміжний стан.
                egui::TextureOptions::NEAREST,
            );
            state.texture = Some((grid.id, texture.clone()));
            texture
        }
    };

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(PLOT_SIDE, PLOT_SIDE),
        egui::Sense::click_and_drag(),
    );
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    // Друга взаємодія з нашим іменем — та сама причина, що в `button`: тест
    // мусить знаходити плот за іменем, а не за підібраним пікселем.
    let named = ui.interact(rect, egui::Id::new(PLOT_IMAGE), egui::Sense::click());

    // Найдешевше вікно позначене хрестиком: плот існує, щоб його знайти, і
    // шукати мінімум оком по градієнту — робота, яку вже зробила машина.
    if let Some((i, j, _)) = grid.best() {
        let at = cell_centre(rect, grid, i, j);
        let arm = 4.0;
        let stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        ui.painter().line_segment(
            [
                egui::pos2(at.x - arm, at.y - arm),
                egui::pos2(at.x + arm, at.y + arm),
            ],
            stroke,
        );
        ui.painter().line_segment(
            [
                egui::pos2(at.x - arm, at.y + arm),
                egui::pos2(at.x + arm, at.y - arm),
            ],
            stroke,
        );
    }

    // Осі: дати по краях замість підписів на кожній поділці. Плот шириною
    // 200 пікселів не витримає більшого, а два кінці вже кажуть, що це за
    // проміжок.
    let (first, last) = (grid.t1[0], grid.t1[grid.t1.len() - 1]);
    ui.label(format!(
        "{} — {}",
        &calendar(first)[..10],
        &calendar(last)[..10]
    ));
    ui.label(format!(
        "{}: {:.1} — {:.1} {}",
        tr(language, Key::FlightTime),
        grid.tof[0] / DAY_S,
        grid.tof[grid.tof.len() - 1] / DAY_S,
        tr(language, Key::Days)
    ));

    // Під курсором — те, на що дивляться; без курсора — те, що обрали.
    let under_pointer = response
        .hover_pos()
        .and_then(|at| cell_at(grid, from_left(rect, at), from_bottom(rect, at)));
    let shown = under_pointer.or(state.chosen);

    match shown.and_then(|(i, j)| read_window(grid, i, j)) {
        Some(readout) => {
            ui.label(format!(
                "{} {}",
                tr(language, Key::Depart),
                calendar(readout.t1)
            ));
            ui.label(format!(
                "{}: {:.2} {}",
                tr(language, Key::FlightTime),
                readout.tof / DAY_S,
                tr(language, Key::Days)
            ));
            match readout.cell {
                Some(cell) => {
                    ui.label(format!(
                        "{}: {:.0} / {:.0} м/с",
                        tr(language, Key::Vinf),
                        cell.v_inf_depart,
                        cell.v_inf_arrive
                    ));
                }
                // Дірка називається дірою. Порожній рядок тут читався б як
                // «безкоштовно».
                None => {
                    ui.label(tr(language, Key::NoSolution));
                }
            }
        }
        None => {
            ui.label(tr(language, Key::PickWindow));
        }
    }

    if named.clicked() {
        if let Some((i, j)) = ui
            .ctx()
            .pointer_interact_pos()
            .and_then(|at| cell_at(grid, from_left(rect, at), from_bottom(rect, at)))
        {
            state.chosen = Some((i, j));
            actions.push(PorkchopAction::Choose(i, j));
        }
    }

    actions
}

fn from_left(rect: egui::Rect, at: egui::Pos2) -> f32 {
    (at.x - rect.min.x) / rect.width()
}

fn from_bottom(rect: egui::Rect, at: egui::Pos2) -> f32 {
    (rect.max.y - at.y) / rect.height()
}

/// Центр клітинки в пікселях — для позначки на плоті.
fn cell_centre(rect: egui::Rect, grid: &Grid, i_t1: usize, i_tof: usize) -> egui::Pos2 {
    let x = (i_t1 as f32 + 0.5) / grid.t1.len() as f32;
    let y = (i_tof as f32 + 0.5) / grid.tof.len() as f32;
    egui::pos2(
        rect.min.x + x * rect.width(),
        rect.max.y - y * rect.height(),
    )
}

/// Сітка в зображення: піксель — клітинка, рядок 0 — найдовший переліт.
///
/// Переворот саме тут, в одному місці: далі його знає лише [`from_bottom`],
/// і обидва підпорядковані одній угоді — `tof` росте вгору.
fn image_of(grid: &Grid) -> egui::ColorImage {
    let (low, high) = grid.range().unwrap_or((0.0, 1.0));
    let (w, h) = (grid.t1.len(), grid.tof.len());

    let mut pixels = vec![egui::Color32::TRANSPARENT; w * h];
    for i in 0..w {
        for j in 0..h {
            let [r, g, b, a] = colour(grid.at(i, j), low, high);
            pixels[(h - 1 - j) * w + i] = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        }
    }

    egui::ColorImage::new([w, h], pixels)
}

/// Числа панелі апарата, зняті зі снапшоту (ROADMAP-UI.md, U2c).
///
/// Окремо від малювання навмисно: оракул кроку — «значення в панелі збігається
/// з незалежно порахованим зі снапшоту», а порівнювати числа з пікселями
/// неможливо. Кожне поле читається [`vessel_panel`], тож структури, яку ніхто
/// не читає, тут немає.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VesselReadout {
    /// Висота над **поверхнею** тіла: відстань від центра мінус середній
    /// радіус з ассета (`eph_body_radius`, U2a). Не над намальованою сферою
    /// і не над опорним радіусом гармонік — для Місяця це різні числа,
    /// і різняться вони на 470 м (K5e).
    pub altitude_m: f64,
    /// Швидкість **відносно тіла**, не барицентрична.
    pub speed_m_s: f64,
    /// Скільки лишилось до наступного маневру плану; `None` — маневрів
    /// попереду немає.
    pub next_burn_s: Option<f64>,
    /// Сумарний Δv плану — **сума норм**, а не норма суми: два маневри
    /// в протилежні боки коштують палива обидва.
    pub total_dv_m_s: f64,
    /// Наскільки прогноз випереджає курсор.
    pub computed_ahead_s: f64,
    /// Апарат зупинився помилкою.
    pub failed: bool,
}

/// Знімає числа апарата зі снапшоту.
///
/// Ефемериду не кличе (правило 5): позиція тіла береться з найближчого
/// семпла, який її вже несе (`leg::Sample::earth` — саме для цього вона там
/// і лежить). Радіус приходить аргументом, бо це властивість тіла, а не
/// кадру: його читають один раз при старті.
pub fn read_vessel(
    snapshot: &WorldSnapshot,
    vessel: &VesselSnapshot,
    body_radius_m: f64,
) -> VesselReadout {
    let body = body_near(vessel, snapshot.t);

    let dr = [
        vessel.state.r.x - body.0[0],
        vessel.state.r.y - body.0[1],
        vessel.state.r.z - body.0[2],
    ];
    let distance = (dr[0] * dr[0] + dr[1] * dr[1] + dr[2] * dr[2]).sqrt();

    let dv = [
        vessel.state.v.x - body.1[0],
        vessel.state.v.y - body.1[1],
        vessel.state.v.z - body.1[2],
    ];
    let speed = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();

    let next_burn_s = vessel
        .plan
        .manoeuvres()
        .iter()
        .find(|m| m.t > snapshot.t)
        .map(|m| m.t - snapshot.t);

    let total_dv_m_s = vessel
        .plan
        .manoeuvres()
        .iter()
        .map(|m| (m.dv[0] * m.dv[0] + m.dv[1] * m.dv[1] + m.dv[2] * m.dv[2]).sqrt())
        .sum();

    VesselReadout {
        altitude_m: distance - body_radius_m,
        speed_m_s: speed,
        next_burn_s,
        total_dv_m_s,
        computed_ahead_s: vessel.computed_to - snapshot.t,
        failed: vessel.failed.is_some(),
    }
}

/// Панель апарата. Нічого не надсилає — U2 лише показує.
pub fn vessel_panel(ui: &mut egui::Ui, language: Language, name: &str, readout: &VesselReadout) {
    ui.heading(tr(language, Key::Vessel));
    ui.label(name);
    ui.label(format!(
        "{}: {:.1} км",
        tr(language, Key::Altitude),
        readout.altitude_m / 1000.0
    ));
    ui.label(format!(
        "{}: {:.1} м/с",
        tr(language, Key::Speed),
        readout.speed_m_s
    ));
    ui.label(match readout.next_burn_s {
        Some(seconds) => format!(
            "{}: {:.2} діб",
            tr(language, Key::NextBurn),
            seconds / DAY_S
        ),
        None => tr(language, Key::NoBurns).to_string(),
    });
    ui.label(format!(
        "{}: {:.2} м/с",
        tr(language, Key::TotalDv),
        readout.total_dv_m_s
    ));
    ui.label(format!(
        "{}: {:.2} діб",
        tr(language, Key::ComputedAhead),
        readout.computed_ahead_s / DAY_S
    ));
    if readout.failed {
        ui.label(tr(language, Key::Failed));
    }
}

/// Позиція й швидкість Землі в найближчому до `t` семплі.
///
/// Семпл несе позицію тіла саме для цього (`crate::leg`), тож ефемерида в
/// кадрі не потрібна. Швидкість семпл не несе, тож вона береться скінченною
/// різницею між двома сусідніми семплами — того самого порядку точності, що
/// й сама лінія на екрані.
fn body_near(vessel: &VesselSnapshot, t: f64) -> ([f64; 3], [f64; 3]) {
    let mut best: Option<(f64, [f64; 3], [f64; 3])> = None;

    for leg in &vessel.legs {
        for (i, sample) in leg.samples.iter().enumerate() {
            let gap = (sample.state.t - t).abs();
            if best.is_some_and(|(was, _, _)| gap >= was) {
                continue;
            }

            // Сусід для різниці: наступний, якщо він є, інакше попередній.
            let velocity = match leg.samples.get(i + 1).or_else(|| {
                if i > 0 {
                    leg.samples.get(i - 1)
                } else {
                    None
                }
            }) {
                Some(other) => {
                    let dt = other.state.t - sample.state.t;
                    if dt == 0.0 {
                        [0.0; 3]
                    } else {
                        [
                            (other.earth[0] - sample.earth[0]) / dt,
                            (other.earth[1] - sample.earth[1]) / dt,
                            (other.earth[2] - sample.earth[2]) / dt,
                        ]
                    }
                }
                None => [0.0; 3],
            };

            best = Some((gap, sample.earth, velocity));
        }
    }

    best.map_or(([0.0; 3], [0.0; 3]), |(_, r, v)| (r, v))
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
