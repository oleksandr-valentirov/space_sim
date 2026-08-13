//! Траєкторія, порахована зараз, а не прочитана з CSV (ROADMAP H5).
//!
//! Це перше місце, де фізика й рендер бачать одне одного. До нього рушій
//! малював фікстури: `data/fixture/halo_inertial.csv` — готовий експорт
//! `core/export/ex_trajectory`, а `engine` навіть не лінкував `core-rs`.
//! Тепер лінкує, і лінія в кадрі — вихід `prop_run`, а не колонка тексту.
//!
//! Що саме тут відбувається, у трьох рядках:
//!
//!   1. стан апарата з першого семпла фікстури — саме заради нього в експорт
//!      повернули `vx,vy,vz`: пропагатору потрібен стан, а не позиція;
//!   2. `core_rs::Propagator` веде його крізь поле десяти тіл ассета,
//!      ланками по буферу — тобто рівно так, як це робитиме гра;
//!   3. на кожен семпл питаємо в ефемериди, де тоді були Земля й Місяць, —
//!      бо саме це малює `trajectory_render`, і саме на цьому стоїть
//!      перетворення фрейму (PROJECT.md §7).
//!
//! ## Чому це не просто «те саме, тільки повільніше»
//!
//! Фікстура — не одна траєкторія. Це розв'язок multiple shooting: сім ланок,
//! кожна проінтегрована зі свого вузла, з розривами 2.3·10⁻² м на швах
//! (ROADMAP C4). Живий прогноз розривів не має за побудовою, і на нестійкій
//! halo-орбіті (594× за оберт) це не дрібниця — саме тому те, як довго дві
//! криві тримаються разом, є вимірюваним твердженням, а не тавтологією.
//! Міряє його `engine/tests/live.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use core_rs::{CoreError, Ephemeris, PropConfig, Propagator, State};

use crate::trajectory::{self, Sample};

/// Ассет ефемериди, від кореня репозиторію.
pub const ASSET: &str = "data/fixture/earth_moon.eph";

/// Той самий ассет абсолютним шляхом, зібраним з `CARGO_MANIFEST_DIR`.
///
/// Це шлях **для зондів і тестів**, а не для гри: `cargo test` запускає
/// бінарник з каталогу крейта, `cargo run` — з того, звідки покликали, і
/// відносний шлях означав би різне в цих двох випадках. Грі шлях до ассетів
/// дасть застосунок, коли він з'явиться; вигадувати для цього шар
/// конфігурації зараз — робота без критерію.
pub fn repo_asset() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine має лежати в репозиторії")
        .join(ASSET)
}

/// Індекси тіл у порядку кукера (`core/cook/cook_fixture.c`).
const EARTH: i32 = 3;
const MOON: i32 = 4;

/// Допуск — той самий сантиметр, з яким `ex_trajectory` рахував фікстуру.
/// Один допуск на прогноз і на фізику (CLAUDE.md, інваріант 5); тут він ще й
/// той самий, що в еталона, інакше порівняння двох кривих міряло б різницю
/// налаштувань замість різниці траєкторій.
const TOL_M: f64 = 1e-2;

/// Стеля кроку. Задана явно, бо з нулем її обирає інтегратор за довжиною
/// ланки — і тоді зшитий прогін лишає по собі інший крок, ніж безперервний
/// (`core/prop.h`, виміряно).
const H_MAX_S: f64 = 3600.0;

/// Скільки семплів забирає один виклик `run`. Навмисно мало: не оптимізація,
/// а те, як це працюватиме в грі — прогноз рахується шматками, між якими
/// можна віддати керування, і шлях зі зшиванням має бути тим, яким ходять
/// щодня, а не рідкісною гілкою, що вперше спрацює під навантаженням.
const LEG: usize = 64;

pub struct Live {
    pub samples: Vec<Sample>,
    /// Скільки викликів `run` знадобилося. Цікаве не саме число, а те, що
    /// воно більше за одиницю: зшивання ланок — не гіпотетичний шлях.
    pub legs: usize,
}

/// Прогноз від стану `start` на `days` діб уперед.
pub fn propagate(start: &State, days: f64, asset: &Path) -> Result<Live, CoreError> {
    let eph = Arc::new(Ephemeris::load(asset)?);

    let cfg = PropConfig {
        tol_m: TOL_M,
        h_max_s: H_MAX_S,
        ..PropConfig::default()
    };
    let mut prop = Propagator::new(eph.clone(), cfg)?;

    let t_end = start.t + days * 86400.0;

    let mut buffer = vec![State::default(); LEG];
    let mut step = 0.0;
    let mut state = *start;
    let mut legs = 0;
    let mut samples = Vec::new();

    loop {
        let run = prop.run(&state, t_end, &[], &mut buffer, &mut step)?;
        legs += 1;

        for s in &buffer[..run.filled] {
            samples.push(Sample {
                t: s.t,
                vessel: [s.r.x, s.r.y, s.r.z],
                velocity: [s.v.x, s.v.y, s.v.z],
                earth: position(&eph, EARTH, s.t)?,
                moon: position(&eph, MOON, s.t)?,
                z_axis: [0.0, 0.0, 0.0],
                // Оракула немає й бути не може: цю траєкторію ніхто не
                // рахував заздалегідь. Синодичні координати рахує сам рендер.
                synodic_reference: [0.0, 0.0, 0.0],
            });
        }

        state = run.final_state;

        if run.stop == core_rs::Stop::ReachedEnd {
            break;
        }
    }

    trajectory::fill_axes(&mut samples);

    Ok(Live { samples, legs })
}

fn position(eph: &Ephemeris, body: i32, t: f64) -> Result<[f64; 3], CoreError> {
    let s = eph.body_state(body, t)?;
    Ok([s.r.x, s.r.y, s.r.z])
}

/// Стан, з якого починається фікстура F6, — і єдине, що з неї береться.
///
/// Це той самий момент і той самий апарат, тож дві криві можна класти поруч.
pub fn fixture_start() -> State {
    let samples = trajectory::load();
    let first = &samples[0];

    State {
        r: core_rs::Vec3d {
            x: first.vessel[0],
            y: first.vessel[1],
            z: first.vessel[2],
        },
        v: core_rs::Vec3d {
            x: first.velocity[0],
            y: first.velocity[1],
            z: first.velocity[2],
        },
        t: first.t,
    }
}
