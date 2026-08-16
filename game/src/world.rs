//! Світ: апарати, їхні траєкторії й той, хто їх рахує (ROADMAP J1).
//!
//! Однонитково — навмисно. Нитка, додана до нерозв'язаної задачі, не
//! розв'язує її, а робить діагностику вдвічі дорожчою; [`World::tick`] тут
//! така сама функція, як будь-яка інша, і J4 лише перенесе її виклик у власну
//! нитку (PROJECT.md §6).
//!
//! ## Що робить тік
//!
//! Тягне горизонт уперед — рівно стільки ланок, скільки йому дозволили, і по
//! колу між апаратами, щоб один не заморив решту. Скільки ланок устигнеться,
//! залежить від машини й від кадру; **де** вони закінчаться — ні. У цьому вся
//! суть: `t_end` приходить із місії, не з годинника (CLAUDE.md, інваріант 9),
//! а буфер під семпли сталий, тож послідовність ланок та сама на будь-якій
//! машині.
//!
//! Курсора часу тут ще немає — він J2. Поки що горизонт просто росте до кінця
//! місії, і цього достатньо, щоб перевірити головне твердження J1: ігрові
//! типи не змінюють жодного біта проти прямого прогону (`tests/trajectory.rs`).

use std::path::Path;
use std::sync::Arc;

use core_rs::{CoreError, Ephemeris, PropConfig, Propagator, State, VesselParams};

use crate::clock::{Clock, Stall};
use crate::leg::{Leg, Sample, Trajectory};
use crate::plan::Plan;
use crate::snapshot::{BodySnapshot, VesselSnapshot, WorldSnapshot};

/// Індекси тіл у порядку кукера (`core/cook/cook_fixture.c`).
pub const SUN: i32 = 0;
pub const EARTH: i32 = 3;
pub const MOON: i32 = 4;

/// Скільки семплів забирає один виклик `prop_run`.
///
/// Число не оптимізоване й не має бути: H1 довів, що воно на траєкторію не
/// впливає взагалі — зшиті ланки бітово дорівнюють одному прогону. Воно
/// впливає лише на зернистість, з якою роботу можна відкласти, і на розмір
/// найменшого шматка, який публікується у снапшоті.
///
/// Навмисно **не** 64, як у `engine::live`: тест J1 звіряє дві траєкторії з
/// різним розміром ланки, і рівність бітова. Однакові числа тут зробили б цю
/// перевірку тавтологією.
pub const LEG: usize = 256;

/// Скільки ланок прогнозу тримати попереду курсора.
///
/// Це і є вся політика горизонту, і вона в **ланках**, а не в секундах —
/// інакше `t_end` походив би від часу, і частота кадрів упливла б у числа
/// (CLAUDE.md, інваріант 9). Скільки це діб, залежить від того, як густо
/// інтегратор ставить кроки: на цій орбіті ланка в 256 семплів — близько
/// одинадцяти діб, тобто чотири ланки дають місяць видимого прогнозу.
///
/// Змінити це число безпечно: воно вирішує, скільки прогнозу існує, і ніколи
/// — які в нього числа.
pub const LEAD_LEGS: usize = 4;

/// Скільки ланок позаду курсора лишаються з усіма сирими семплами (N3a).
///
/// Ланками, а не добами: ланка — одиниця всього (CLAUDE.md), і на низькій
/// орбіті вона покриває півтори доби, а на місячному перельоті одинадцять.
/// Вікно в ланках підлаштовується під режим само, вікно в добах — ні.
///
/// Чотири — стільки ж, скільки [`LEAD_LEGS`] попереду: вікно навколо курсора
/// симетричне, і жодна з двох сторін не має підстав бути ширшою.
pub const RAW_LEGS_BEHIND: usize = 4;

/// Скільки обертів позаду курсора лишається в історії (N5a, рішення Q4).
///
/// **Оберти, а не доби й не мегабайти.** Доба оманлива — 5100 семплів на LEO
/// проти 720 на місячному перельоті, — а обертами вікно підлаштовується під
/// режим само. Мегабайти гравець не може співвіднести ні з чим; він мислить
/// витками.
///
/// Двадцять — щоб слід читався як слід, а не як відрізок, і щоб на низькій
/// орбіті це була доба з гаком. Це **значення за замовчуванням**, а не стеля:
/// вікно живе в полі світу (`World::set_history_revolutions`), бо колись його
/// крутитиме гравець. Скільки це пам'яті, каже `Trajectory::history_bytes`, і
/// саме це число має стояти поруч із вибором в інтерфейсі.
///
/// ⚠ **Двері в один бік** (інваріант 5): викинута ланка не повертається, тож
/// це не налаштування продуктивності, а вибір того, скільки минулого гравець
/// хоче бачити.
pub const HISTORY_REVOLUTIONS: f64 = 20.0;

/// Допуск, з яким ланка йде на пенсію, метри (N3a).
///
/// **Виведений, а не обраний**, і виведений з масштабу, на якому карта
/// відкривається: `mission::CAMERA_ALTITUDE_M` = 10⁹ м, `focal_px` при 720p —
/// 623 пікселі на радіан, отже пів пікселя це `10⁹ · 0.5 / 623 ≈ 8·10⁵` м.
///
/// ⚠ **Двері в один бік, і ось де вони видні.** Наблизившись до старого
/// минулого ближче, ніж на 10⁹ м, гравець побачить хорди замість дуг — семплів
/// між ними більше немає й не буде (інваріант 5). Це ціна припущення Q4, і
/// саме тому число живе тут, з викладкою, а не в тілі функції.
pub const RETIRE_TOL_M: f64 = 8.0e5;

/// Індекс апарата у `Vec<Vessel>` (CLAUDE.md: індекси замість посилань).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VesselId(pub u32);

pub struct Vessel {
    pub id: VesselId,
    pub name: String,

    /// Стан, з якого продовжувати. Кінець порахованого, а не «зараз».
    pub tip: State,
    /// Крок інтегратора, з яким продовжувати. Йде в сейв (PROJECT.md §4).
    pub tip_step: f64,
    /// Кінець місії: далі не рахуємо взагалі.
    pub horizon_end: f64,

    /// План маневрів. Порожній — вільний політ.
    pub plan: Plan,
    /// Скільки маневрів плану вже вшито в траєкторію.
    ///
    /// Індекс, а не час: порівнювати часи довелося б із точністю, якої в
    /// плаваючої коми немає, а лічильник каже це однозначно. Після
    /// каскадного перерахунку він перераховується з нуля
    /// ([`World::commit_plan`]).
    pub applied: usize,

    pub trajectory: Trajectory,

    /// Площа, маса й коефіцієнт відбиття — усе, що потрібно тиску світла
    /// (ROADMAP K6b). `None` — безмасова пробна частинка, як було до K6b.
    ///
    /// Задається при створенні й далі не змінюється. Це не забудькуватість:
    /// зміна моделі сил на льоту зробила б уже пораховану частину прогнозу
    /// траєкторією, якою апарат не полетить, тобто вимагала б каскадного
    /// перерахунку — того самого, що робить правка плану. Площа апарата не
    /// змінюється, а маса змінюється при горінні, якого імпульсна модель
    /// маневрів не має.
    pub params: Option<VesselParams>,

    /// Чому горизонт перестав рости. Ядро віддає помилки кодами, і найгірше,
    /// що можна з ними зробити, — впасти: світ лишається валідним, просто
    /// цей апарат далі не рахується.
    pub failed: Option<CoreError>,
}

impl Vessel {
    /// Чи є ще що рахувати при курсорі в `cursor`.
    ///
    /// Три умови, і кожна вимикає роботу з іншої причини: апарат зламався,
    /// місія скінчилася, або прогнозу вже достатньо далеко попереду.
    fn wants_work(&self, cursor: f64) -> bool {
        self.failed.is_none()
            && self.tip.t < self.horizon_end
            && self.trajectory.legs_after(cursor) < LEAD_LEGS
    }

    /// Докуди інтегрувати наступною ланкою.
    ///
    /// Наступний незастосований маневр або кінець місії — і ніколи не
    /// курсор (CLAUDE.md, інваріант 9). Саме тут план перетворюється на
    /// послідовність викликів `prop_run`: кожен сегмент між маневрами
    /// проходиться ланками, а межа сегмента стає `t_end`.
    fn next_boundary(&self) -> f64 {
        match self.plan.get(self.applied) {
            Some(m) if m.t < self.horizon_end => m.t,
            _ => self.horizon_end,
        }
    }

    /// Докуди пораховано.
    fn computed_to(&self) -> f64 {
        self.trajectory.computed_to()
    }
}

pub struct World {
    eph: Arc<Ephemeris>,
    /// Один пропагатор на конфігурацію, а не на апарат: контекст у C тримає
    /// налаштування, а стан апарата — наш (`core/prop.h`). Він `Send`, але не
    /// `Sync`, тож належить рівно одній нитці — тій, що кличе `tick`.
    prop: Propagator,
    vessels: Vec<Vessel>,
    /// Курсор часу. Пишеться лише тут (PROJECT.md §6).
    clock: Clock,
    /// Скільки разів світ змінювався. Читач снапшоту з нього бачить, що
    /// картинка нова, не порівнюючи вміст.
    version: u64,
    /// Скільки ланок пораховано за весь час. Не статистика: цим міряється
    /// вартість каскадного перерахунку (`tests/plan.rs`).
    legs_computed: u64,
    /// Скільки ланок позаду курсора лишати сирими (`set_history_trimming`).
    retire_behind: Option<usize>,
    /// Скільки обертів історії тримати (`set_history_revolutions`).
    history_revolutions: f64,
}

/// Чому план не прийнято.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRejected {
    NoSuchVessel,
    /// Зміна торкається моменту, який курсор уже пройшов.
    ///
    /// Це не обмеження зручності, а те, на чому стоїть недоторканність
    /// історії: правити можна лише майбутнє, отже переписуються лише ланки
    /// прогнозу (PROJECT.md §6).
    InThePast,
}

/// Чому перемотування не прийнято (ROADMAP-UI.md, U3b).
///
/// Відмова саме відмова, а не тихе ігнорування: правило 8 етапу U вимагає, щоб
/// панель показала відповідь, а не власне припущення про успіх.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeekRejected {
    /// Курсор не ходить назад ніколи (J-етап).
    Backwards,
    /// Туди ще не пораховано; `computed_to` — докуди можна.
    NotComputedYet { computed_to: f64 },
}

/// Що зробив один тік.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tick {
    /// Скільки ланок пораховано.
    pub legs: usize,
    /// Чи лишилась робота: горизонт когось із апаратів не дійшов до кінця.
    pub pending: bool,
    /// Скільки семплів пішло на пенсію на цьому тіку (N3a).
    pub retired: usize,
    /// Скільки семплів вийшло за вікно й зникло на цьому тіку (N5a).
    pub dropped: usize,
}

impl World {
    pub fn new(asset: &Path, cfg: PropConfig, epoch: f64, warp: f64) -> Result<World, CoreError> {
        World::with_ephemeris(Arc::new(Ephemeris::load(asset)?), cfg, epoch, warp)
    }

    /// Те саме, але на вже завантаженій ефемериді.
    ///
    /// Потрібно планувальнику (J5): його спекулятивний світ ділить ассет із
    /// справжнім. Ділити його можна тому, що `Ephemeris` — `Sync`, і це не
    /// припущення, а прочитаний C (`core-rs`, D3): контекст після
    /// `eph_load` не змінюється взагалі.
    pub fn with_ephemeris(
        eph: Arc<Ephemeris>,
        cfg: PropConfig,
        epoch: f64,
        warp: f64,
    ) -> Result<World, CoreError> {
        let prop = Propagator::new(eph.clone(), cfg)?;

        Ok(World {
            eph,
            prop,
            vessels: Vec::new(),
            clock: Clock::new(epoch, warp),
            version: 0,
            legs_computed: 0,
            retire_behind: Some(RAW_LEGS_BEHIND),
            history_revolutions: HISTORY_REVOLUTIONS,
        })
    }

    /// Чи чіпати минуле взагалі, і скільки ланок позаду курсора лишати
    /// сирими (N3a, N5a).
    ///
    /// Одна ручка на дві дії, бо вони одна політика: вікно в
    /// [`HISTORY_REVOLUTIONS`] обертів **викидає** ланки, старші за нього, а
    /// пенсія **проріджує** те, що лишилось далі, ніж `behind_legs` ланок від
    /// курсора. `None` вимикає обидві.
    ///
    /// Політика, а не налаштування, і в неї два законні значення в самому
    /// проєкті. Гра ріже: інакше пам'ять росте так, як каже D7. Не ріжуть
    /// двоє — спекулятивний світ планувальника, який живе кілька ланок і
    /// встиг би хіба заплатити за прохід, і будь-яка перевірка, що звіряє
    /// **потік семплів** із незалежним прогоном: різання змінює те, що
    /// така перевірка порівнює, не змінюючи жодного біта того, що порахували.
    pub fn set_history_trimming(&mut self, behind_legs: Option<usize>) {
        self.retire_behind = behind_legs;
    }

    /// Скільки обертів минулого лишається в історії (N5a, рішення Q4).
    ///
    /// ⚠ Зменшення — двері в один бік: викинуті ланки не повертаються
    /// (інваріант 5). Збільшення діє лише на майбутнє.
    pub fn set_history_revolutions(&mut self, revolutions: f64) {
        self.history_revolutions = revolutions;
    }

    pub fn ephemeris(&self) -> Arc<Ephemeris> {
        self.eph.clone()
    }

    pub fn add_vessel(
        &mut self,
        name: &str,
        start: State,
        horizon_end: f64,
        params: Option<VesselParams>,
    ) -> VesselId {
        // Нуль означає «обери сам» лише на першому виклику; далі переноситься
        // те, що лишив попередній (`core/prop.h`).
        self.add_planned_vessel(name, start, 0.0, horizon_end, Plan::new(), params)
    }

    /// Апарат, що продовжує чужий політ: із заданим кроком і вже заданим
    /// планом.
    ///
    /// Це шлях планувальника (J5). `step` тут не косметика: прогноз, що
    /// починається з «обери сам», — це інша траєкторія, ніж продовження з
    /// перенесеним кроком (H1), тобто саме те, чого прев'ю не має права
    /// показувати.
    pub fn add_planned_vessel(
        &mut self,
        name: &str,
        start: State,
        step: f64,
        horizon_end: f64,
        plan: Plan,
        params: Option<VesselParams>,
    ) -> VesselId {
        let id = VesselId(self.vessels.len() as u32);
        self.vessels.push(Vessel {
            id,
            name: name.to_string(),
            tip: start,
            tip_step: step,
            horizon_end,
            plan,
            applied: 0,
            trajectory: Trajectory::new(start),
            params,
            failed: None,
        });

        let index = self.vessels.len() - 1;
        bake_applied(&self.eph, &mut self.vessels[index]);

        self.version += 1;
        id
    }

    /// Апарат із сейву: усе задано явно, нічого не виводиться.
    ///
    /// Головна відмінність від [`World::add_planned_vessel`] — `applied`
    /// **береться**, а не рахується. Маневр рівно в момент `tip` уже
    /// застосований (його Δv у `tip`), але з чисел цього не видно: стан до й
    /// після імпульсу мають однаковий час. Вивести його тут означало б
    /// виконати маневр удруге при кожному завантаженні (`crate::save`).
    ///
    /// Бере [`crate::save::SavedVessel`] цілком, а не сім аргументів. Ця
    /// структура вже описує рівно те, що треба відновити, і два списки полів
    /// поруч розійшлися б мовчки — саме так `params` (K6b) і був би
    /// загублений при завантаженні.
    pub fn add_saved_vessel(&mut self, saved: crate::save::SavedVessel) -> VesselId {
        let id = VesselId(self.vessels.len() as u32);
        self.vessels.push(Vessel {
            id,
            name: saved.name,
            tip: saved.tip,
            tip_step: saved.step,
            horizon_end: saved.horizon_end,
            plan: saved.plan,
            applied: saved.applied,
            trajectory: Trajectory::new(saved.tip),
            params: saved.params,
            failed: None,
        });
        self.version += 1;
        id
    }

    pub fn vessels(&self) -> &[Vessel] {
        &self.vessels
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Скільки ланок пораховано за весь час життя світу.
    pub fn legs_computed(&self) -> u64 {
        self.legs_computed
    }

    /// Приймає новий план для апарата й перераховує лише те, що після зміни.
    ///
    /// Повертає момент, з якого перерахунок почався, або `None`, якщо план не
    /// змінився.
    ///
    /// **Історія тут не переписується — і не тому, що ми обережні.** Правки в
    /// минулому відхиляються, тож усе, що курсор уже пройшов, за побудовою
    /// лежить у ланках, яких зміна не торкається.
    pub fn commit_plan(&mut self, id: VesselId, plan: Plan) -> Result<Option<f64>, PlanRejected> {
        let cursor = self.clock.t();
        let vessel = self
            .vessels
            .get_mut(id.0 as usize)
            .ok_or(PlanRejected::NoSuchVessel)?;

        let Some(from) = vessel.plan.diverges_from(&plan) else {
            return Ok(None);
        };
        if from <= cursor {
            return Err(PlanRejected::InThePast);
        }

        let restart = vessel.trajectory.truncate_after(from);
        vessel.tip = restart.state;
        vessel.tip_step = restart.step;
        vessel.plan = plan;
        // Новий план — нова спроба: попередній міг упертися в межу ассета
        // саме тим маневром, який щойно прибрали.
        vessel.failed = None;

        // Маневри, раніші за точку перезапуску, вже вшиті в збережені семпли.
        // Той, що припадає рівно на неї, — ні: ланка закінчується станом ДО
        // імпульсу, а сам імпульс жив у `tip`, який ми щойно перезаписали.
        bake_applied(&self.eph, vessel);

        self.version += 1;
        Ok(Some(from))
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn clock_mut(&mut self) -> &mut Clock {
        &mut self.clock
    }

    /// Один крок світу: спершу порахувати, потім рушити час.
    ///
    /// Порядок саме такий, і він не косметичний. Спершу курсор означало б, що
    /// на високому warp час упирається в горизонт, який цього ж кадру мав би
    /// вирости, — гра «затиналася» б через кадр при цілком достатній
    /// пропускній здатності.
    ///
    /// Перевести курсор на вже пораховану мить (ROADMAP-UI.md, U3b).
    ///
    /// **Нічого не інтегрує.** Це рух курсора по тому, що вже пораховано, і
    /// саме тому перемотування до події не змінює жодного біта траєкторії:
    /// перевірка кроку в тому, що `legs_computed()` після нього не зросло.
    ///
    /// Відмовляє двічі, і обидві відмови названі:
    ///
    /// - **назад курсор не ходить ніколи** (та сама причина, що в
    ///   `Clock::advance`: сейв інакше стрибав би в минуле). Гравець має
    ///   побачити, що гра цього не вміє, а не подумати, що промахнувся мишею;
    /// - **уперед — не далі, ніж пораховано.** Інакше «перемотати» означало б
    ///   «порахувати», тобто `t_end` від інтерфейсу — прямо проти інваріанта 9.
    pub fn seek_to(&mut self, t: f64) -> Result<(), SeekRejected> {
        if t < self.clock.t() {
            return Err(SeekRejected::Backwards);
        }

        let limit = self
            .vessels
            .iter()
            .filter(|v| v.failed.is_none())
            .map(Vessel::computed_to)
            .fold(f64::INFINITY, f64::min);

        if !limit.is_finite() || t > limit {
            return Err(SeekRejected::NotComputedYet { computed_to: limit });
        }

        self.clock.seek_to(t);
        Ok(())
    }

    /// `dt_wall` — секунди реального часу, аргументом. Світ не читає годинник
    /// сам, і саме тому цю функцію можна прогнати з будь-якою послідовністю
    /// кадрів і звірити біти (`tests/time.rs`).
    pub fn step(&mut self, dt_wall: f64, budget: usize) -> Tick {
        let mut done = self.tick(budget);

        // Пенсія — після роботи, а не перед: ланка, яку щойно порахували,
        // виходить за вікно не раніше, ніж з'явиться четверта після неї.
        if let Some(window) = self.retire_behind {
            let cursor = self.clock.t();
            for vessel in &mut self.vessels {
                // Спершу вікно, тоді пенсія: викидати дешевше, ніж проріджувати
                // те, що зараз викинеш.
                done.dropped += vessel
                    .trajectory
                    .keep_revolutions(cursor, self.history_revolutions);
                done.retired += vessel.trajectory.retire_before(window, RETIRE_TOL_M);
            }
        }

        let cursor_limit = self
            .vessels
            .iter()
            .filter(|v| v.failed.is_none())
            .map(Vessel::computed_to)
            .fold(f64::INFINITY, f64::min);

        let mission_end = self
            .vessels
            .iter()
            .filter(|v| v.failed.is_none())
            .map(|v| v.horizon_end)
            .fold(f64::NEG_INFINITY, f64::max);

        // Світ без живих апаратів не має чим обмежувати курсор — і не має
        // куди його вести. Хай стоїть.
        if cursor_limit.is_finite() {
            self.clock.advance(dt_wall, cursor_limit, mission_end);
        }

        done
    }

    /// Тягне горизонт уперед, не більше ніж `budget` ланок.
    ///
    /// По колу між апаратами: інакше перший у списку з'їдав би весь бюджет,
    /// і дев'ятий апарат гравця не рахувався б ніколи.
    pub fn tick(&mut self, budget: usize) -> Tick {
        let mut done = Tick::default();
        let cursor = self.clock.t();

        while done.legs < budget {
            let mut worked = false;

            for index in 0..self.vessels.len() {
                if done.legs >= budget {
                    break;
                }
                if !self.vessels[index].wants_work(cursor) {
                    continue;
                }

                // Рахується поступ, а не спроба. Без цієї різниці апарат, який
                // чомусь не рухається, крутив би цикл вічно — і не впав би, а
                // завис, що набагато гірше. У правильному коді такого не буває
                // (`extend` завжди або додає ланку, або виконує маневр), тож
                // це сторож, а не механізм. Знайдено перевіркою зубів:
                // вимкнений маневр перетворював прогін на вічний цикл.
                if self.extend(index) {
                    done.legs += 1;
                    worked = true;
                }
            }

            // Нікому не було чого рахувати — бюджет не витрачаємо на порожні
            // оберти циклу.
            if !worked {
                break;
            }
        }

        if done.legs > 0 {
            self.version += 1;
        }
        done.pending = self.vessels.iter().any(|v| v.wants_work(cursor));
        done
    }

    /// Те саме, але до заданого моменту, а не до кінця місії.
    ///
    /// Курсор ведеться тим самим `step`, а не ставиться присвоєнням: стан, у
    /// який гра не може потрапити грою, не варто вміти показувати.
    pub fn run_to_day(&mut self, until: f64, dt_wall: f64, budget: usize) -> usize {
        let mut steps = 0;
        while self.clock.t() < until {
            let before = self.clock.t();
            let done = self.step(dt_wall, budget);
            steps += 1;

            if self.clock.stall() == Some(Stall::MissionEnd) {
                break;
            }
            if done.legs == 0 && self.clock.t() == before {
                break;
            }
        }
        steps
    }

    /// Проганяє місію до кінця: рахує й веде курсор, доки той не стане.
    ///
    /// Це не ігровий режим, а зручність для того, кому потрібна вся місія
    /// одразу: знімка без вікна й тестів. `dt_wall` тут великий навмисно —
    /// час усе одно впирається в горизонт, і саме так перевіряється, що
    /// впирається він правильно.
    pub fn run_to_end(&mut self, dt_wall: f64, budget: usize) -> usize {
        let mut steps = 0;
        loop {
            let before = self.clock.t();
            let done = self.step(dt_wall, budget);
            steps += 1;

            if self.clock.stall() == Some(Stall::MissionEnd) {
                return steps;
            }
            // Сторож проти вічного циклу: ніхто нічого не порахував і час не
            // зрушив — далі не зрушить теж.
            if done.legs == 0 && self.clock.t() == before {
                return steps;
            }
        }
    }

    /// Одна ланка одного апарата. Повертає, чи був поступ.
    fn extend(&mut self, index: usize) -> bool {
        let vessel = &mut self.vessels[index];

        let mut buffer = vec![State::default(); LEG];
        let entry = vessel.tip;
        let boundary = vessel.next_boundary();

        // t_end з плану або з місії, не з годинника. Ланка закінчиться або
        // тут, або на заповненому буфері — обидві межі відтворювані.
        let run = match self.prop.run(
            &vessel.tip,
            vessel.params.as_ref(),
            boundary,
            &[],
            &mut buffer,
            &mut vessel.tip_step,
        ) {
            Ok(run) => run,
            Err(e) => {
                vessel.failed = Some(e);
                return false;
            }
        };

        buffer.truncate(run.filled);

        let mut samples = Vec::with_capacity(buffer.len());
        for state in buffer {
            let (earth, moon) = match (
                position(&self.eph, EARTH, state.t),
                position(&self.eph, MOON, state.t),
            ) {
                (Ok(earth), Ok(moon)) => (earth, moon),
                (Err(e), _) | (_, Err(e)) => {
                    vessel.failed = Some(e);
                    return false;
                }
            };
            samples.push(Sample { state, earth, moon });
        }

        vessel.tip = run.final_state;

        // Ланка без жодного семпла буває рівно в одному випадку: два маневри
        // в один момент, коли `prop_run` не має куди інтегрувати. Зберігати
        // її нема сенсу, а зламала б вона багато — від інтерполяції до
        // порівняння меж.
        let mut progressed = false;
        if !samples.is_empty() {
            self.legs_computed += 1;
            progressed = true;
            vessel.trajectory.push(Leg {
                entry,
                t1: run.final_state.t,
                step_out: vessel.tip_step,
                samples,
                stop: run.stop,
            });
        }

        // Дійшли рівно до маневру — виконуємо його. Порівняння точне, і це
        // не необережність: `prop.c` пише `t_end` у кінцевий стан дослівно й
        // саме за цією рівністю відрізняє `CORE_STOP_T_END`.
        if run.stop == core_rs::Stop::ReachedEnd {
            if let Some(m) = vessel.plan.get(vessel.applied) {
                if m.t == vessel.tip.t {
                    apply_manoeuvre(&self.eph, vessel);
                    progressed = true;
                }
            }
        }

        progressed
    }

    /// Незмінний зріз світу для читачів.
    ///
    /// У J1 його одразу ж і споживають на тій самій нитці; типом він уже той,
    /// яким його публікуватиме `arc-swap` у J4, і саме тому будується він тут,
    /// а не в рендері — щоб межа існувала до того, як з'явиться нитка.
    pub fn snapshot(&self) -> WorldSnapshot {
        let t = self.clock.t();

        WorldSnapshot {
            version: self.version,
            t,
            warp: self.clock.warp(),
            stall: self.clock.stall(),
            bodies: self.bodies_at(t),
            // Сонце — окремим полем, а не серед тіл: див. `WorldSnapshot::sun`.
            sun: self
                .eph
                .body_state(SUN, t)
                .ok()
                .map(|state| [state.r.x, state.r.y, state.r.z]),
            vessels: self
                .vessels
                .iter()
                .map(|v| {
                    // Інтерполяція робиться тут, а не в рендері, і це не
                    // економія: два споживачі, кожен зі своїм `state_at`,
                    // бачили б два різні «зараз» в одному кадрі. З тієї ж
                    // причини `C` рахується з **цього** стану, а не з другої
                    // інтерполяції.
                    let state = v.trajectory.state_at(t);
                    VesselSnapshot {
                        id: v.id,
                        name: v.name.clone(),
                        state,
                        jacobi: self.jacobi_at(t, &state),
                        legs: v.trajectory.share(),
                        plan: v.plan.clone(),
                        start: v.trajectory.start(),
                        tip: v.tip,
                        computed_to: v.computed_to(),
                        horizon_end: v.horizon_end,
                        params: v.params,
                        failed: v.failed,
                    }
                })
                .collect(),
        }
    }

    /// Константа Якобі апарата в синодичному фреймі пари (ROADMAP-UI.md, U6b3).
    ///
    /// Один виклик ефемериди на снапшот, у нитці світу — рівно там, де вже
    /// рахуються тіла. Помилка означає «фрейму немає», а не нуль: нуль — це
    /// теж значення `C`, і воно намалювало б криву не там.
    fn jacobi_at(&self, t: f64, state: &State) -> Option<f64> {
        let frame = self.eph.synodic_frame(EARTH, MOON, t).ok()?;
        let synodic = frame.from_inertial(state);
        Some(core_rs::cr3bp_jacobi(
            synodic.r,
            synodic.v,
            frame.mass_ratio(),
        ))
    }

    /// Тіла, які видно в кадрі, у момент `t` (ROADMAP-PLANETS.md, R1c).
    ///
    /// **Рахується тут, у нитці світу, а не в кадрі** — і це те саме рішення,
    /// що вже зроблене для `state_at`: два споживачі, кожен зі своїм викликом
    /// ефемериди, бачили б два різні «зараз» в одному кадрі. Плюс правило 5
    /// етапу U: панель і рендер ефемериду не кличуть.
    ///
    /// Радіус і орієнтація приходять із ассета — розмір і поворот Землі не
    /// властивості рушія. Помилку ефемериди тут ковтати нема куди й нема
    /// навіщо: тіло без моделі обертання й так віддає одиничний кватерніон, а
    /// час поза проміжком ассета для курсора неможливий — його не пускає
    /// горизонт. Тому невдача читається як «тіла в кадрі немає», і це видно.
    fn bodies_at(&self, t: f64) -> Vec<BodySnapshot> {
        [EARTH, MOON]
            .iter()
            .filter_map(|&body| {
                let state = self.eph.body_state(body, t).ok()?;
                let q = self.eph.body_orientation(body, t).ok()?;
                Some(BodySnapshot {
                    body,
                    position: [state.r.x, state.r.y, state.r.z],
                    velocity: [state.v.x, state.v.y, state.v.z],
                    radius_m: self.eph.body_radius(body),
                    mu: self.eph.body_mu(body),
                    orientation: [q.w, q.x, q.y, q.z],
                })
            })
            .collect()
    }
}

/// Рахує, скільки маневрів плану вже вшито в стан `vessel.tip`.
///
/// Раніші за `tip.t` — вшиті в збережені семпли. Той, що припадає рівно на
/// нього, — ні: ланка закінчується станом ДО імпульсу, а сам імпульс жив у
/// `tip`, який щойно перезаписали (або якого ще не було зовсім).
fn bake_applied(eph: &Ephemeris, vessel: &mut Vessel) {
    vessel.applied = 0;
    while let Some(m) = vessel.plan.get(vessel.applied).copied() {
        if m.t < vessel.tip.t {
            vessel.applied += 1;
        } else if m.t == vessel.tip.t {
            // Сам інкремент робить `apply_manoeuvre`.
            apply_manoeuvre(eph, vessel);
        } else {
            break;
        }
    }
}

/// Виконує наступний незастосований маневр над `vessel.tip`.
///
/// Вільна функція, а не метод: їй потрібні водночас ефемерида світу й
/// мутабельний апарат, а це два поля однієї структури.
///
/// Помилка ефемериди тут не губиться, а зупиняє апарат: маневр, виконаний з
/// фреймом «нуль», був би тихо не тим маневром.
fn apply_manoeuvre(eph: &Ephemeris, vessel: &mut Vessel) {
    let Some(m) = vessel.plan.get(vessel.applied).copied() else {
        return;
    };

    let body = match m.frame_body() {
        Some(id) => match eph.body_state(id, vessel.tip.t) {
            Ok(state) => Some(state),
            Err(e) => {
                vessel.failed = Some(e);
                return;
            }
        },
        None => None,
    };

    let dv = m.dv_inertial(&vessel.tip, body.as_ref());
    vessel.tip.v.x += dv[0];
    vessel.tip.v.y += dv[1];
    vessel.tip.v.z += dv[2];
    vessel.applied += 1;
}

fn position(eph: &Ephemeris, body: i32, t: f64) -> Result<[f64; 3], CoreError> {
    let s = eph.body_state(body, t)?;
    Ok([s.r.x, s.r.y, s.r.z])
}
