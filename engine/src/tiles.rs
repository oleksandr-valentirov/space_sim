//! Тайли рельєфу: формат і читач (ROADMAP-PLANETS.md, R5b).
//!
//! PROJECT.md §7 забороняє завантажувати сирі формати в рантаймі, тож між
//! LOLA й кадром стоїть кукер (`tools/dem-cook`). Формат — власний, з версією
//! в заголовку, рівно як в ассета ефемериди.
//!
//! ## Чому формат живе тут, а не в кукері
//!
//! Записувач і читач одного формату мусять бути **одним кодом**, інакше вони
//! розійдуться — не одразу, а на четвертій правці. Напрямок залежності при
//! цьому визначений однозначно: кукер уже потребує геометрії кубосфери
//! (`crate::cubesphere`), тож `dem-cook → engine`, а не навпаки. Рушій про
//! кукер не знає нічого.
//!
//! ## Тайл — це патч, вузол у вузол
//!
//! Тайл зберігає висоту **в тих самих вузлах**, які має сітка патча:
//! `(SIDE + 1)²` значень. Не текстуру з довільним розміром, не сітку з
//! запасом по краю — рівно вузли.
//!
//! Наслідок, заради якого так і зроблено: **тріщин рельєф не додає.** Вершина
//! на спільному ребрі двох патчів бітово одна (R2b), тож напрямок, за яким
//! кукер брав висоту, теж бітово один, тож і висота одна — у двох сусідніх
//! тайлах лежить те саме число. Зшивання рівнів (`cubesphere::indices`)
//! працює далі без єдиної правки: воно викидає непарний вузол, а парний
//! однаково лежить в обох тайлах.
//!
//! ## Піраміда, і де вона закінчується
//!
//! Тайли кукаються на рівні від 0 до [`Terrain::levels`] − 1. Глибше йти
//! нема сенсу — джерело скінченне, — і патч глибшого рівня бере висоту з
//! тайла свого предка, білінійно. Це не наближення заради дешевизни: тайл
//! **дрібніший за клітинку джерела** нічого нового не містить, він лише
//! коштує пам'яті.
//!
//! ## Висоти — `int16`, без стиснення
//!
//! Розвилка була названа наперед: BC4 для висот дає видимі сходинки на
//! пологих схилах. Тайл малий (33×33 = 2178 байтів), тож стискати його —
//! міняти видиму якість на кілобайти. Стиснення кольору (BC7/BC6H) — інша
//! задача й інший крок.

use crate::cubesphere::{Patch, FACES, SIDE};

/// Підпис файлу. Вісім байтів, щоб заголовок читався оком у hex-дампі.
pub const MAGIC: [u8; 8] = *b"SSDEM\0\0\0";

/// Версія формату. Росте при будь-якій зміні розкладки — читач старої версії
/// мусить сказати про це, а не прочитати сміття.
pub const VERSION: u32 = 1;

/// Скільки вузлів на бік тайла — стільки ж, скільки в сітки патча.
pub const NODES: usize = SIDE + 1;

/// Скільки байтів займає один тайл: дві межі плюс сітка.
const TILE_BYTES: usize = 4 + NODES * NODES * 2;

/// Заголовок: підпис, версія, три числа й радіус.
const HEADER_BYTES: usize = 8 + 4 + 4 + 4 + 8 + 4;

/// Рельєф одного тіла — піраміда тайлів по патчах кубосфери.
#[derive(Clone, Debug)]
pub struct Terrain {
    /// Скільки рівнів піраміди, від 0 включно.
    pub levels: u32,
    /// Опорний радіус, від якого відлічуються висоти, метри.
    pub reference_m: f64,
    /// Скільки метрів в одиниці зберігання.
    pub scale_m: f32,
    /// Тайли підряд у канонічному порядку — див. [`Terrain::index`].
    tiles: Vec<u8>,
}

impl Terrain {
    /// Скільки тайлів має рівень `level`.
    fn per_level(level: u32) -> usize {
        FACES << (2 * level)
    }

    /// Скільки тайлів у піраміді з `levels` рівнями.
    pub fn count(levels: u32) -> usize {
        (0..levels).map(Terrain::per_level).sum()
    }

    /// Порядковий номер тайла: рівень за рівнем, у кожному — грань за гранню,
    /// у кожній — рядок за рядком.
    ///
    /// Порядок сталий і виводиться з самого патча, без таблиці: інакше
    /// кукер і читач мали б два способи дійти до одного числа.
    pub fn index(&self, patch: &Patch) -> Option<usize> {
        if patch.level >= self.levels {
            return None;
        }
        let before: usize = (0..patch.level).map(Terrain::per_level).sum();
        let side = 1usize << patch.level;
        Some(before + (patch.face * side + patch.i as usize) * side + patch.j as usize)
    }

    /// Патч, чий тайл накриває цей патч: він сам або найближчий предок у
    /// піраміді.
    ///
    /// Разом із ним — у скільки разів тайл грубіший, тобто на скільки треба
    /// поділити локальні координати.
    pub fn covering(&self, patch: &Patch) -> (Patch, u32) {
        let mut it = *patch;
        while it.level >= self.levels {
            it = it.parent().expect("рівень 0 завжди в піраміді");
        }
        (it, patch.level - it.level)
    }

    /// Межі висот тайла в одиницях зберігання: найнижча й найвища.
    ///
    /// Це те, чого R3a чекав від тайлів: радіус затуляння міряється від
    /// **найнижчої** точки, а не від середнього радіуса тіла.
    pub fn bounds(&self, index: usize) -> (i16, i16) {
        let at = index * TILE_BYTES;
        (
            i16::from_le_bytes([self.tiles[at], self.tiles[at + 1]]),
            i16::from_le_bytes([self.tiles[at + 2], self.tiles[at + 3]]),
        )
    }

    /// Висота вузла тайла в одиницях зберігання.
    pub fn node(&self, index: usize, a: usize, b: usize) -> i16 {
        let at = index * TILE_BYTES + 4 + (a * NODES + b) * 2;
        i16::from_le_bytes([self.tiles[at], self.tiles[at + 1]])
    }

    /// Сирі байти одного тайла — те, що поїде в текстуру (R5c).
    pub fn tile_bytes(&self, index: usize) -> &[u8] {
        let at = index * TILE_BYTES;
        &self.tiles[at + 4..at + TILE_BYTES]
    }

    /// Куди патч дивиться в тайлі, який його накриває (R7a).
    ///
    /// Три числа, і саме їх бракувало GPU-шляху: індекс тайла в піраміді,
    /// зсув патча всередині нього **у вузлах** і крок — скільки вузлів
    /// предка припадає на один вузол патча. При `deeper == 0` це `(0, 0)` і
    /// `1.0`, тобто рівно те, що робив точний `Load` до цього кроку.
    ///
    /// Винесено з [`Terrain::height_m`], а не написано поруч: шейдер робить
    /// ту саму вибірку, і два незалежні обчислення того самого вікна
    /// розійшлися б — не одразу, а на четвертій правці. Тепер це одна
    /// формула, і тест звіряє GPU з нею, а не з другою її копією.
    pub fn window(&self, patch: &Patch) -> (usize, [f64; 2], f64) {
        let (tile, deeper) = self.covering(patch);
        let index = self.index(&tile).expect("covering вже опустив рівень");

        // `SIDE` вузлів предка на `2^deeper` дітей — отже крок дробовий.
        let step = 1.0 / f64::from(1u32 << deeper);
        let offset = |index: u32| f64::from(index % (1 << deeper)) * SIDE as f64 * step;
        (index, [offset(patch.i), offset(patch.j)], step)
    }

    /// Висота у вузлі `(a, b)` заданого патча, метри.
    ///
    /// Якщо патч глибший за піраміду, висота береться з тайла предка
    /// білінійно: вузол патча лежить між вузлами грубішого тайла.
    pub fn height_m(&self, patch: &Patch, a: usize, b: usize) -> f64 {
        let (index, origin, step) = self.window(patch);
        if step == 1.0 {
            return f64::from(self.node(index, a, b)) * f64::from(self.scale_m);
        }

        let x = origin[0] + a as f64 * step;
        let y = origin[1] + b as f64 * step;

        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let (x0, y0) = (x0 as usize, y0 as usize);
        let get = |dx: usize, dy: usize| {
            f64::from(self.node(index, (x0 + dx).min(SIDE), (y0 + dy).min(SIDE)))
        };
        let top = get(0, 0) * (1.0 - ty) + get(0, 1) * ty;
        let bottom = get(1, 0) * (1.0 - ty) + get(1, 1) * ty;
        (top * (1.0 - tx) + bottom * tx) * f64::from(self.scale_m)
    }

    /// Найнижча точка всього рельєфу, метри над опорним радіусом.
    ///
    /// Рівень 0 накриває тіло цілком, тож обходити всю піраміду не треба.
    pub fn lowest_m(&self) -> f64 {
        let mut low = i16::MAX;
        for index in 0..Terrain::per_level(0) {
            low = low.min(self.bounds(index).0);
        }
        f64::from(low) * f64::from(self.scale_m)
    }

    /// Зібрати набір із готових тайлів — шлях кукера.
    ///
    /// Тайли подаються в канонічному порядку; функція лише перевіряє, що їх
    /// стільки, скільки має бути, і рахує межі кожного.
    pub fn build(levels: u32, reference_m: f64, scale_m: f32, grids: &[Vec<i16>]) -> Terrain {
        assert_eq!(
            grids.len(),
            Terrain::count(levels),
            "тайлів не стільки, скільки має бути в піраміді з {levels} рівнями"
        );
        let mut tiles = Vec::with_capacity(grids.len() * TILE_BYTES);
        for grid in grids {
            assert_eq!(grid.len(), NODES * NODES, "тайл не тієї форми");
            let low = grid.iter().copied().min().unwrap_or(0);
            let high = grid.iter().copied().max().unwrap_or(0);
            tiles.extend_from_slice(&low.to_le_bytes());
            tiles.extend_from_slice(&high.to_le_bytes());
            for value in grid {
                tiles.extend_from_slice(&value.to_le_bytes());
            }
        }
        Terrain {
            levels,
            reference_m,
            scale_m,
            tiles,
        }
    }

    /// Байти файлу.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BYTES + self.tiles.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(NODES as u32).to_le_bytes());
        out.extend_from_slice(&self.levels.to_le_bytes());
        out.extend_from_slice(&self.reference_m.to_le_bytes());
        out.extend_from_slice(&self.scale_m.to_le_bytes());
        out.extend_from_slice(&self.tiles);
        out
    }

    /// Розібрати байти файлу.
    pub fn from_bytes(bytes: &[u8]) -> Result<Terrain, String> {
        if bytes.len() < HEADER_BYTES {
            return Err(format!("{} байтів — це навіть не заголовок", bytes.len()));
        }
        if bytes[..8] != MAGIC {
            return Err("не той підпис: це не тайлсет".to_string());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let version = word(8);
        if version != VERSION {
            return Err(format!(
                "версія формату {version}, а цей рушій читає {VERSION}"
            ));
        }
        let nodes = word(12) as usize;
        if nodes != NODES {
            return Err(format!(
                "тайл на {nodes} вузлів, а патч має {NODES} — сітки не збігаються"
            ));
        }
        let levels = word(16);
        let reference_m = f64::from_le_bytes(bytes[20..28].try_into().unwrap());
        let scale_m = f32::from_le_bytes(bytes[28..32].try_into().unwrap());

        let tiles = bytes[HEADER_BYTES..].to_vec();
        let wanted = Terrain::count(levels) * TILE_BYTES;
        if tiles.len() != wanted {
            return Err(format!(
                "{} байтів тайлів замість {wanted} на {levels} рівнів",
                tiles.len()
            ));
        }

        Ok(Terrain {
            levels,
            reference_m,
            scale_m,
            tiles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Піраміда, у якій висота вузла — відома функція його позиції на грані.
    ///
    /// Лінійна за обома координатами навмисно: білінійна вибірка відтворює
    /// лінійну функцію **точно**, тож будь-яка розбіжність у тесті нижче — це
    /// помилка адресації, а не похибка інтерполяції. Оракул, який плутає одне
    /// з одним, не оракул.
    fn ramp(levels: u32) -> Terrain {
        let mut grids = Vec::with_capacity(Terrain::count(levels));
        for level in 0..levels {
            let side = 1u32 << level;
            // Грань не входить у висоту: рампа однакова на всіх шести, і саме
            // тому вона ловить помилку адресації, а не маскує її гранню.
            for _face in 0..FACES {
                for i in 0..side {
                    for j in 0..side {
                        // Вузол у частках грані — величина, однакова на всіх
                        // рівнях. Саме це робить піраміду узгодженою: тайли
                        // рівня 0 і рівня 2 в тій самій точці грані несуть те
                        // саме число, як і має бути в справжній піраміді.
                        //
                        // Множники підібрані так, щоб значення лишалися
                        // цілими на вузлах кожного рівня: інакше округлення до
                        // `i16` саме створило б розбіжність, яку тест шукає.
                        let span = (SIDE << level) as f64;
                        let mut grid = Vec::with_capacity(NODES * NODES);
                        for a in 0..NODES {
                            for b in 0..NODES {
                                let x = (i as usize * SIDE + a) as f64 / span;
                                let y = (j as usize * SIDE + b) as f64 / span;
                                grid.push((2048.0 * x + 4096.0 * y) as i16);
                            }
                        }
                        grids.push(grid);
                    }
                }
            }
        }
        Terrain::build(levels, 1_000_000.0, 1.0, &grids)
    }

    /// Патч, глибший за піраміду, у вузлі предка дає **рівно** висоту предка.
    ///
    /// Це найпростіше твердження про адресацію, і воно ловить найгрубішу
    /// помилку: вікно, зсунуте не туди, дасть тут сусідній вузол, а не той
    /// самий.
    #[test]
    fn a_node_that_coincides_with_the_parent_reads_the_parents_height() {
        let levels = 3;
        let terrain = ramp(levels);

        // Рівень 3 при трьох рівнях піраміди (0..2) — рівно на один глибше.
        let deep = Patch {
            face: 2,
            level: levels,
            i: 5,
            j: 3,
        };
        let (parent, deeper) = terrain.covering(&deep);
        assert_eq!(deeper, 1, "предок мав бути рівно на рівень вище");

        // Вузол (0,0) патча завжди збігається з вузлом предка.
        let from_deep = terrain.height_m(&deep, 0, 0);
        let (index, origin, step) = terrain.window(&deep);
        assert_eq!(step, 0.5, "крок для одного рівня глибше — половина вузла");

        let (x, y) = (origin[0] as usize, origin[1] as usize);
        let direct = f64::from(terrain.node(index, x, y)) * f64::from(terrain.scale_m);
        assert_eq!(
            from_deep, direct,
            "вузол (0,0) глибокого патча прочитався не з того місця предка \
             (предок {parent:?}, вікно {origin:?})"
        );
    }

    /// Спільна точка двох сусідніх глибоких патчів дає ту саму висоту.
    ///
    /// Це і є вимога «тріщин рельєф не додає», перенесена на рівні, глибші за
    /// дані. Перевіряються **обидва** випадки, і другий важливіший:
    ///
    /// 1. сусіди всередині одного предка — тут працює сама арифметика вікна;
    /// 2. сусіди по різні боки межі предків — тут вікна різні, тайли різні, і
    ///    висота збігається лише тому, що вузол на спільному ребрі лежить в
    ///    обох тайлах (R5b). Помилка на одиницю у зсуві ламає саме цей випадок,
    ///    а перший лишає зеленим.
    #[test]
    fn neighbouring_deep_patches_agree_on_their_shared_edge() {
        let levels = 3;
        let terrain = ramp(levels);
        let level = levels + 1; // на два рівні глибше за піраміду

        let at = |i: u32, j: u32| Patch {
            face: 1,
            level,
            i,
            j,
        };

        // `i` росте разом із координатою `a`, тож ребро між (i, j) і (i+1, j) —
        // це вузли a = SIDE у лівого й a = 0 у правого.
        let pairs = [
            // Обидва всередині одного тайла предка: 4 дітей на предка при
            // deeper = 2, отже 0..3 — один предок.
            (0u32, 1u32),
            // Через межу предків: 3 і 4 належать різним тайлам.
            (3, 4),
        ];

        for (left, right) in pairs {
            let (l_index, _, _) = terrain.window(&at(left, 0));
            let (r_index, _, _) = terrain.window(&at(right, 0));
            let across_parents = l_index != r_index;

            for b in [0usize, 7, SIDE / 2, SIDE] {
                let from_left = terrain.height_m(&at(left, 0), SIDE, b);
                let from_right = terrain.height_m(&at(right, 0), 0, b);
                assert_eq!(
                    from_left, from_right,
                    "патчі {left} і {right} розійшлись на спільному ребрі у \
                     вузлі b = {b} (різні предки: {across_parents})"
                );
            }
        }
    }

    /// Вікно самого патча, який має власний тайл, — тотожне.
    ///
    /// Сторож проти регресії в інший бік: якщо `window` почне зсувати те, що
    /// зсувати не треба, рельєф поїде на всіх звичайних рівнях одразу, а не
    /// лише на глибоких.
    #[test]
    fn a_patch_with_its_own_tile_reads_itself_unchanged() {
        let terrain = ramp(3);
        for level in 0..3 {
            let patch = Patch {
                face: 4,
                level,
                i: 0,
                j: 0,
            };
            let (index, origin, step) = terrain.window(&patch);
            assert_eq!(origin, [0.0, 0.0], "власний тайл зсунувся");
            assert_eq!(step, 1.0, "власний тайл змінив крок");
            assert_eq!(index, terrain.index(&patch).expect("тайл є"));
        }
    }
}
