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

use core_rs::{CoreError, Ephemeris, PropConfig, Propagator, State};

use crate::leg::{Leg, Sample, Trajectory};
use crate::snapshot::{VesselSnapshot, WorldSnapshot};

/// Індекси тіл у порядку кукера (`core/cook/cook_fixture.c`).
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
    /// Докуди рахувати. J1: кінець місії; J3: наступний маневр.
    pub horizon_end: f64,

    pub trajectory: Trajectory,

    /// Чому горизонт перестав рости. Ядро віддає помилки кодами, і найгірше,
    /// що можна з ними зробити, — впасти: світ лишається валідним, просто
    /// цей апарат далі не рахується.
    pub failed: Option<CoreError>,
}

impl Vessel {
    /// Чи є ще що рахувати.
    fn wants_work(&self) -> bool {
        self.failed.is_none() && self.tip.t < self.horizon_end
    }
}

pub struct World {
    eph: Arc<Ephemeris>,
    /// Один пропагатор на конфігурацію, а не на апарат: контекст у C тримає
    /// налаштування, а стан апарата — наш (`core/prop.h`). Він `Send`, але не
    /// `Sync`, тож належить рівно одній нитці — тій, що кличе `tick`.
    prop: Propagator,
    vessels: Vec<Vessel>,
    /// Скільки разів світ змінювався. Читач снапшоту з нього бачить, що
    /// картинка нова, не порівнюючи вміст.
    version: u64,
}

/// Що зробив один тік.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tick {
    /// Скільки ланок пораховано.
    pub legs: usize,
    /// Чи лишилась робота: горизонт когось із апаратів не дійшов до кінця.
    pub pending: bool,
}

impl World {
    pub fn new(asset: &Path, cfg: PropConfig) -> Result<World, CoreError> {
        let eph = Arc::new(Ephemeris::load(asset)?);
        let prop = Propagator::new(eph.clone(), cfg)?;

        Ok(World {
            eph,
            prop,
            vessels: Vec::new(),
            version: 0,
        })
    }

    pub fn add_vessel(&mut self, name: &str, start: State, horizon_end: f64) -> VesselId {
        let id = VesselId(self.vessels.len() as u32);
        self.vessels.push(Vessel {
            id,
            name: name.to_string(),
            tip: start,
            // Нуль означає «обери сам» лише на першому виклику; далі
            // переноситься те, що лишив попередній (`core/prop.h`).
            tip_step: 0.0,
            horizon_end,
            trajectory: Trajectory::default(),
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

    /// Тягне горизонт уперед, не більше ніж `budget` ланок.
    ///
    /// По колу між апаратами: інакше перший у списку з'їдав би весь бюджет,
    /// і дев'ятий апарат гравця не рахувався б ніколи.
    pub fn tick(&mut self, budget: usize) -> Tick {
        let mut done = Tick::default();

        while done.legs < budget {
            let mut worked = false;

            for index in 0..self.vessels.len() {
                if done.legs >= budget {
                    break;
                }
                if !self.vessels[index].wants_work() {
                    continue;
                }

                self.extend(index);
                done.legs += 1;
                worked = true;
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
        done.pending = self.vessels.iter().any(Vessel::wants_work);
        done
    }

    /// Тікає, доки є що рахувати. Повертає кількість тіків.
    ///
    /// Це не ігровий режим, а зручність для того, кому потрібен увесь прогноз
    /// одразу: знімка без вікна й тестів. У вікні тік викликають по кадру.
    pub fn run_to_horizon(&mut self, budget: usize) -> usize {
        let mut ticks = 0;
        loop {
            let done = self.tick(budget);
            ticks += 1;
            // `legs == 0` — сторож проти вічного циклу: якщо апарат уперся в
            // помилку, `pending` стане хибним, але покладатися варто на обидва.
            if !done.pending || done.legs == 0 {
                return ticks;
            }
        }
    }

    /// Одна ланка одного апарата.
    fn extend(&mut self, index: usize) {
        let vessel = &mut self.vessels[index];

        let mut buffer = vec![State::default(); LEG];
        let t0 = vessel.tip.t;

        // t_end з місії, не з годинника. Ланка закінчиться або тут, або на
        // заповненому буфері — обидві межі відтворювані.
        let run = match self.prop.run(
            &vessel.tip,
            vessel.horizon_end,
            &[],
            &mut buffer,
            &mut vessel.tip_step,
        ) {
            Ok(run) => run,
            Err(e) => {
                vessel.failed = Some(e);
                return;
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
                    return;
                }
            };
            samples.push(Sample { state, earth, moon });
        }

        vessel.tip = run.final_state;
        vessel.trajectory.push(Leg {
            t0,
            t1: run.final_state.t,
            step_out: vessel.tip_step,
            samples,
            stop: run.stop,
        });
    }

    /// Незмінний зріз світу для читачів.
    ///
    /// У J1 його одразу ж і споживають на тій самій нитці; типом він уже той,
    /// яким його публікуватиме `arc-swap` у J4, і саме тому будується він тут,
    /// а не в рендері — щоб межа існувала до того, як з'явиться нитка.
    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            version: self.version,
            vessels: self
                .vessels
                .iter()
                .map(|v| VesselSnapshot {
                    id: v.id,
                    name: v.name.clone(),
                    legs: v.trajectory.share(),
                    tip: v.tip,
                    failed: v.failed,
                })
                .collect(),
        }
    }
}

fn position(eph: &Ephemeris, body: i32, t: f64) -> Result<[f64; 3], CoreError> {
    let s = eph.body_state(body, t)?;
    Ok([s.r.x, s.r.y, s.r.z])
}
