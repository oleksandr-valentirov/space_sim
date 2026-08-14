//! Незмінний зріз світу (ROADMAP J1, PROJECT.md §6).
//!
//! Це те, що з J4 публікуватиме `arc-swap`, і тому воно існує вже зараз, коли
//! нитка одна: межа, проведена після появи потоку, проходить там, де зручно
//! потоку, а не там, де правильно. Тут вона проходить по «неперервний стан».
//!
//! **Чого тут немає й не буде: подій.** Снапшот — це вибірка; читач, що
//! пропустив публікацію, пропустив би подію назавжди. Дискретне ходить
//! каналом (CLAUDE.md, інваріант 8), і в J4 це буде окремий тип.

use std::sync::Arc;

use core_rs::{CoreError, State};

use crate::leg::Leg;
use crate::world::VesselId;

pub struct VesselSnapshot {
    pub id: VesselId,
    pub name: String,

    /// Ланки як вони є. Клон цього вектора — це клон вказівників: ланка
    /// незмінна від моменту, коли її порахували, тож ділити її безпечно й
    /// нічого не коштує.
    pub legs: Vec<Arc<Leg>>,

    /// Кінець порахованого. Не «де апарат зараз» — курсор часу приходить у J2.
    pub tip: State,

    pub failed: Option<CoreError>,
}

impl VesselSnapshot {
    pub fn sample_count(&self) -> usize {
        self.legs.iter().map(|leg| leg.samples.len()).sum()
    }
}

pub struct WorldSnapshot {
    pub version: u64,
    pub vessels: Vec<VesselSnapshot>,
}
