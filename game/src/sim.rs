//! Нитка симуляції (ROADMAP J4, PROJECT.md §6).
//!
//! Світ переїжджає у власну нитку, і код світу від цього не змінюється
//! жодним рядком: [`world::World::step`](crate::world::World::step) як була
//! звичайною функцією, так і лишилась. Змінюється тільки те, **хто** її
//! кличе — і саме тому J1–J3 робилися однонитково.
//!
//! ## Два примітиви, і третього немає
//!
//! - **Канал** ([`Command`]) — усе, що головна нитка хоче зробити зі світом.
//! - **Публікація** ([`arc_swap`]) — усе, що вона хоче про світ знати.
//!
//! Спільного мутабельного стану немає взагалі, тож гонка тут неможлива не
//! тому, що ми обережні, а тому, що немає чого замикати. Читач ніколи не
//! блокує письменника: `ArcSwap::load_full` — це атомарний обмін
//! вказівниками, а не очікування.
//!
//! ## Чому події теж каналом
//!
//! [`Event`] — те, що сталося **один раз**: план прийнято, план відхилено,
//! апарат уперся в межу ассета. Снапшот такого не переносить: він вибірка,
//! і читач, який пропустив публікацію, пропустив би подію назавжди
//! (CLAUDE.md, інваріант 8).
//!
//! ## Що нитка НЕ змінює
//!
//! Числа. Нитка вимірює власний `dt` і крутиться зі своїм тіком, тобто робить
//! рівно те, що J2 уже перевірив як безпечне: міняє швидкість курсора й
//! кількість роботи за прохід, ніколи — `t_end`. Перевірка на це є
//! (`tests/thread.rs`): траєкторія з нитки бітово дорівнює однонитковій.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use core_rs::CoreError;
use crossbeam_channel::{Receiver, Sender};

use crate::mission;
use crate::plan::Plan;
use crate::snapshot::WorldSnapshot;
use crate::world::{PlanRejected, VesselId, World};

/// Період тіку симуляції.
///
/// Удвічі частіше за кадр при 60 Hz: снапшот не має бути свіжішим за кадр, але
/// має бути не старішим. Це не крок фізики — його не існує (`crate::clock`), —
/// а лише те, як часто нитка прокидається сама. Команда будить її негайно.
const TICK: Duration = Duration::from_millis(8);

/// Скільки ланок дозволено порахувати за один тік.
///
/// Стеля затримки, не оптимізація: чим більше число, тим довше нитка не
/// дивиться в канал команд. На числа не впливає (інваріант 9).
const LEGS_PER_TICK: usize = 4;

/// Стеля на `dt` одного тіку. Та сама причина, що в `app`: процес, приспаний
/// системою на хвилину, не має прокидатися з хвилиною × warp у руках.
const MAX_TICK_DT: f64 = 0.25;

/// Що головна нитка просить у симуляції.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetWarp(f64),
    ScaleWarp(f64),
    TogglePause,
    CommitPlan { vessel: VesselId, plan: Plan },
    Shutdown,
}

/// Що симуляція повідомляє назад. Дискретне — тобто те, чого снапшот не
/// переносить.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// План прийнято; `from` — момент, з якого перераховано.
    PlanCommitted {
        vessel: VesselId,
        from: Option<f64>,
    },
    PlanRejected {
        vessel: VesselId,
        why: PlanRejected,
    },
    /// Апарат перестав рахуватися. Найімовірніша причина — час вийшов за
    /// проміжок ассета.
    VesselFailed {
        vessel: VesselId,
        error: CoreError,
    },
}

/// Ручка до нитки симуляції.
///
/// Володіє ниткою: [`Drop`] просить її спинитися й чекає. Без цього процес
/// із закритим вікном лишався б із живою ниткою, яка й далі рахує.
pub struct Sim {
    commands: Sender<Command>,
    events: Receiver<Event>,
    published: Arc<ArcSwap<WorldSnapshot>>,
    thread: Option<JoinHandle<()>>,
}

impl Sim {
    /// Піднімає світ і нитку під нього.
    ///
    /// Світ будується **тут**, у нитці-викликачі, а не всередині: помилка
    /// завантаження ассета має долетіти до того, хто її може показати, а не
    /// вбити нитку, яка ще нікому не відома.
    pub fn spawn(asset: PathBuf, demo_plan: bool) -> Result<Sim, String> {
        let mut world = build(&asset, demo_plan)?;

        let published = Arc::new(ArcSwap::from_pointee(world.snapshot()));
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (event_tx, events) = crossbeam_channel::unbounded();

        let publish = published.clone();
        let thread = std::thread::Builder::new()
            .name("sim".to_string())
            .spawn(move || run(&mut world, &command_rx, &event_tx, &publish))
            .map_err(|e| format!("нитка симуляції не запустилася: {e}"))?;

        Ok(Sim {
            commands,
            events,
            published,
            thread: Some(thread),
        })
    }

    /// Поточний зріз світу.
    ///
    /// **Один виклик на кадр, і тримати результат весь кадр.** Два виклики
    /// підряд можуть дати два різні «зараз», і тоді камера дивитиметься на
    /// одну мить, а траєкторія малюватиметься з іншої.
    pub fn snapshot(&self) -> Arc<WorldSnapshot> {
        self.published.load_full()
    }

    pub fn send(&self, command: Command) {
        // Канал закривається лише разом із ниткою, тобто в `Drop`. Помилка
        // тут означала б, що нитка впала; світ від цього не псується, а
        // повідомити нема кому — UI ще немає.
        let _ = self.commands.send(command);
    }

    /// Забирає всі події, що накопичилися. Не блокує.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn build(asset: &std::path::Path, demo_plan: bool) -> Result<World, String> {
    let build = if demo_plan {
        mission::world_with_demo_plan
    } else {
        mission::world
    };
    build(asset).map_err(|e| format!("світ не будується ({}): {e}", asset.display()))
}

/// Цикл нитки.
///
/// Прокидається або від команди, або від тіку — `select!` саме заради
/// першого: пауза за натисканням пробілу не має чекати до кінця періоду.
fn run(
    world: &mut World,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    published: &ArcSwap<WorldSnapshot>,
) {
    let ticker = crossbeam_channel::tick(TICK);
    let mut last = Instant::now();
    let mut reported_failure = vec![false; world.vessels().len()];

    loop {
        crossbeam_channel::select! {
            recv(commands) -> command => {
                match command {
                    // Відправник зник разом із `Sim` — виходимо так само, як
                    // на Shutdown.
                    Err(_) => return,
                    Ok(Command::Shutdown) => return,
                    Ok(command) => apply(world, command, events),
                }
            }
            recv(ticker) -> _ => {}
        }

        // Решта команд, що встигли накопичитися, — щоб серія натискань не
        // розтягувалася на серію тіків.
        while let Ok(command) = commands.try_recv() {
            if command == Command::Shutdown {
                return;
            }
            apply(world, command, events);
        }

        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64();
        last = now;

        world.step(dt.min(MAX_TICK_DT), LEGS_PER_TICK);

        // Про поломку доповідаємо один раз на апарат: канал не має
        // перетворюватися на потік того самого повідомлення щотіку.
        reported_failure.resize(world.vessels().len(), false);
        for (index, vessel) in world.vessels().iter().enumerate() {
            if let Some(error) = vessel.failed {
                if !reported_failure[index] {
                    reported_failure[index] = true;
                    let _ = events.send(Event::VesselFailed {
                        vessel: vessel.id,
                        error,
                    });
                }
            } else {
                reported_failure[index] = false;
            }
        }

        published.store(Arc::new(world.snapshot()));
    }
}

fn apply(world: &mut World, command: Command, events: &Sender<Event>) {
    match command {
        Command::SetWarp(warp) => world.clock_mut().set_warp(warp),
        Command::ScaleWarp(factor) => world.clock_mut().scale_warp(factor),
        Command::TogglePause => world.clock_mut().toggle_pause(),
        Command::CommitPlan { vessel, plan } => {
            let event = match world.commit_plan(vessel, plan) {
                Ok(from) => Event::PlanCommitted { vessel, from },
                Err(why) => Event::PlanRejected { vessel, why },
            };
            let _ = events.send(event);
        }
        // Оброблено вище: сюди воно не доходить.
        Command::Shutdown => {}
    }
}
