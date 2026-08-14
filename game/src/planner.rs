//! Нитка планувальника: спекулятивні прогони (ROADMAP J5, PROJECT.md §6).
//!
//! PROJECT.md §6 називає цю нитку `Planner`, і поділ у неї не «фізика проти
//! прогнозу», а **закомічене проти спекулятивного**. Прогноз і фізика — одна
//! інтеграція (§4, правило 5), тож дві нитки, що інтегрують один апарат, були
//! б тим самим апаратом двічі. А от «покажи, що буде, якщо палити отут» —
//! інша задача: її результат можна викинути, її можна скасувати на півдорозі,
//! і в світ вона не пише нічого.
//!
//! ## Обіцянка, заради якої все це існує
//!
//! > Лінія, яку ви бачили, і є лінія, якою полетите.
//!
//! Тобто прев'ю мусить бути **бітово** тим, що потім порахує `Sim`. Це не
//! випливає само: досить почати прогін не з тієї точки або з «обери крок
//! сам», і прев'ю розійдеться з польотом (H1 виміряв, наскільки).
//!
//! Тому тут немає власного сегментного циклу. Планувальник будує звичайний
//! [`World`] на тій самій ефемериді, з тим самим `PropConfig`, і кличе той
//! самий `step`. Точку перезапуску обидва беруть з однієї функції
//! ([`crate::leg::restart_at`]). Спільний код — не заощадження, а сама
//! обіцянка: дві реалізації розійшлися б, і розійшлися б непомітно.
//!
//! ## Скасування
//!
//! Гравець тягне вузол маневру — запити летять десятками за секунду, і всі,
//! крім останнього, нікому не потрібні. Нитка кидає роботу **між ланками**,
//! щойно в каналі з'явився новіший запит: чекати кінця прогону, який уже не
//! питають, означало б відставати від миші рівно на один прогін.

use std::sync::Arc;
use std::thread::JoinHandle;

use core_rs::{Ephemeris, PropConfig, State};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::leg::Leg;
use crate::plan::Plan;
use crate::world::{VesselId, World};

/// Скільки ланок рахувати між перевірками каналу.
///
/// Одна: ланка — це вже одиниця роботи, і робити скасування грубішим означало
/// б відповідати із затримкою в цілу ланку без жодного виграшу.
const LEGS_PER_CHECK: usize = 1;

/// Що порахувати.
///
/// `from` і `step` — точка перезапуску, порахована [`crate::leg::restart_at`]
/// зі снапшоту. Саме з неї `Sim` перерахує хвіст, коли план закомітять, — і
/// саме тому прев'ю мусить починатися звідти ж, а не з «де апарат зараз».
#[derive(Debug, Clone)]
pub struct Request {
    pub id: u64,
    pub vessel: VesselId,
    pub from: State,
    pub step: f64,
    pub plan: Plan,
    /// Кінець місії апарата — той самий, що в світі.
    pub horizon_end: f64,
    /// Апарат так, як його бачить модель сил (K6b). Мусить бути той самий,
    /// що у світі: прев'ю з іншою площею — це лінія, якою апарат не полетить,
    /// тобто рівно те, чого ця нитка не має права показувати.
    pub params: Option<core_rs::VesselParams>,
}

/// Порахований прогноз. Не стан світу: його ніхто нікуди не поклав.
pub struct Preview {
    pub id: u64,
    pub vessel: VesselId,
    pub plan: Plan,
    pub legs: Vec<Arc<Leg>>,
}

pub struct Planner {
    /// `Option` заради [`Drop`]: щоб нитка вийшла, відправника треба
    /// **знищити**, а не просто перестати ним користуватися.
    requests: Option<Sender<Request>>,
    previews: Receiver<Preview>,
    thread: Option<JoinHandle<()>>,
}

impl Planner {
    /// Піднімає нитку на **тій самій** ефемериді, що й світ.
    ///
    /// Ділити її можна тому, що `Ephemeris` — `Sync`, і це доведено читанням
    /// C ще в D3, задовго до того, як знадобилося. Пропагатор натомість у
    /// кожної нитки свій: він `Send`, але не `Sync`.
    pub fn spawn(eph: Arc<Ephemeris>, cfg: PropConfig) -> Result<Planner, String> {
        let (requests, request_rx) = crossbeam_channel::unbounded::<Request>();
        let (preview_tx, previews) = crossbeam_channel::unbounded();

        let thread = std::thread::Builder::new()
            .name("planner".to_string())
            .spawn(move || run(&eph, cfg, &request_rx, &preview_tx))
            .map_err(|e| format!("нитка планувальника не запустилася: {e}"))?;

        Ok(Planner {
            requests: Some(requests),
            previews,
            thread: Some(thread),
        })
    }

    pub fn request(&self, request: Request) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(request);
        }
    }

    /// Найсвіжіше прев'ю, якщо воно є. Старіші відкидаються.
    ///
    /// Відкидає **тут**, а не в нитці: нитка не знає, який запит викликач
    /// вважає актуальним, а викликач знає — це той, що він послав останнім.
    pub fn latest(&self) -> Option<Preview> {
        self.previews.try_iter().last()
    }
}

impl Drop for Planner {
    fn drop(&mut self) {
        // Закритий канал запитів — це і є сигнал виходу: нитці нема від кого
        // чекати роботи. Окремої команди `Shutdown` тут не треба, бо, на
        // відміну від `Sim`, планувальник нічого не робить без запиту.
        self.requests = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    eph: &Arc<Ephemeris>,
    cfg: PropConfig,
    requests: &Receiver<Request>,
    previews: &Sender<Preview>,
) {
    while let Ok(first) = requests.recv() {
        // Поки ми спали, могло прилетіти ще кілька. Актуальний — останній.
        let mut request = first;
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }

        if let Some(preview) = compute(eph, cfg, &request, requests) {
            if previews.send(preview).is_err() {
                return;
            }
        }
    }
}

/// Рахує прогноз, кидаючи роботу, щойно прилетів новіший запит.
///
/// `None` означає «скасовано» — і саме тому воно `Option`, а не порожній
/// результат: порожнє прев'ю викликач намалював би.
fn compute(
    eph: &Arc<Ephemeris>,
    cfg: PropConfig,
    request: &Request,
    requests: &Receiver<Request>,
) -> Option<Preview> {
    // Звичайнісінький світ. Той самий код, той самий `step`, той самий
    // `PropConfig` — і саме тому результат бітово збігається з тим, що
    // порахує `Sim`.
    let mut world = World::with_ephemeris(eph.clone(), cfg, request.from.t, 1.0).ok()?;
    let vessel = world.add_planned_vessel(
        "preview",
        request.from,
        request.step,
        request.horizon_end,
        request.plan.clone(),
        request.params,
    );

    loop {
        match requests.try_recv() {
            // Новіший запит або зниклий канал — цей результат уже не
            // потрібен.
            Ok(_) | Err(TryRecvError::Disconnected) => return None,
            Err(TryRecvError::Empty) => {}
        }

        // Курсор не рухаємо (`dt = 0`): прев'ю — це прогноз, а не політ.
        let done = world.step(0.0, LEGS_PER_CHECK);
        if done.legs == 0 {
            break;
        }
    }

    let vessel = &world.vessels()[vessel.0 as usize];
    Some(Preview {
        id: request.id,
        vessel: request.vessel,
        plan: request.plan.clone(),
        legs: vessel.trajectory.share(),
    })
}
