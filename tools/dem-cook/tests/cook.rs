//! Кукер: той самий вхід — той самий байт, і те саме число двома шляхами (R5b).
//!
//! Два твердження, і жодне з них не про красу тайла.
//!
//! **Перше — стабільність.** Ассет, який щоразу інший, ламає все, що на ньому
//! стоїть: звірку хешів, кеш збірки, `git diff`. Тут вона не постульована, а
//! перевірена двома прогонами поспіль.
//!
//! **Друге — та сама форма оракула, що в K5e: два шляхи, одне число.** Висота
//! в тайлі й висота, прочитана з джерела за широтою й довготою, мусять
//! збігтися. Шляхи справді різні: кукер іде через `Patch::vertex` і
//! `sample_direction_m`, тест — через явний переклад напрямку в градуси й
//! `sample_m`. Помилка в кубосфері зсунула б перше й не зачепила другого.

use dem_cook::cook::build;
use dem_cook::Grid;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles::Terrain;
use std::path::Path;

const LEVELS: u32 = 3;

fn grid() -> Grid {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/lola/ldem_4.img");
    Grid::read(&path).expect("сітка LOLA мала прочитатися")
}

/// Дешевий стабільний хеш байтів — FNV-1a. Криптографії тут не треба:
/// питання не «чи підробили», а «чи те саме».
fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Два прогони кукера дають байт у байт те саме.
#[test]
fn cooking_twice_gives_the_same_bytes() {
    let grid = grid();
    let first = build(&grid, LEVELS).to_bytes();
    let second = build(&grid, LEVELS).to_bytes();

    println!(
        "  {} рівнів, {} тайлів, {} байтів, хеш {:016x}",
        LEVELS,
        Terrain::count(LEVELS),
        first.len(),
        digest(&first)
    );
    assert_eq!(
        digest(&first),
        digest(&second),
        "два прогони дали різні байти"
    );
    assert_eq!(first, second);
}

/// Файл читається назад у те, що в нього поклали.
#[test]
fn the_file_survives_a_round_trip() {
    let grid = grid();
    let terrain = build(&grid, LEVELS);
    let back = Terrain::from_bytes(&terrain.to_bytes()).expect("файл мав прочитатися");

    assert_eq!(back.levels, terrain.levels);
    assert_eq!(back.scale_m, terrain.scale_m);
    assert_eq!(back.reference_m, terrain.reference_m);
    assert_eq!(back.to_bytes(), terrain.to_bytes());

    // Чужа версія мусить сказати про себе, а не прочитатися сміттям.
    let mut broken = terrain.to_bytes();
    broken[8] = 99;
    assert!(
        Terrain::from_bytes(&broken).is_err(),
        "чужу версію прийняли"
    );
    broken[0] = b'X';
    assert!(
        Terrain::from_bytes(&broken).is_err(),
        "чужий підпис прийняли"
    );
}

/// Висота в тайлі й висота з джерела — одне число, двома шляхами.
#[test]
fn the_tile_agrees_with_the_source_read_another_way() {
    let grid = grid();
    let terrain = build(&grid, LEVELS);

    let mut worst: f64 = 0.0;
    let mut checked = 0;
    for level in 0..LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    // Центр тайла й три його кути — там, де помилка в осях
                    // грані вилізла б найпомітніше.
                    for (a, b) in [(SIDE / 2, SIDE / 2), (0, 0), (0, SIDE), (SIDE, 0)] {
                        let d = patch.vertex(a, b, 1.0);
                        // Інший шлях: напрямок → градуси → `sample_m`.
                        let lat = d[2].atan2((d[0] * d[0] + d[1] * d[1]).sqrt());
                        let lon = d[1].atan2(d[0]);
                        let from_source = grid.sample_m(lat, lon);
                        let from_tile = terrain.height_m(&patch, a, b);
                        worst = worst.max((from_tile - from_source).abs());
                        checked += 1;
                    }
                }
            }
        }
    }

    println!("  {checked} точок; найбільша розбіжність {worst:.4} м");
    // Півкванта зберігання — усе, що дозволено: округлення до 0.5 м і
    // нічого понад це.
    assert!(
        worst <= f64::from(terrain.scale_m) / 2.0 + 1e-9,
        "тайл розійшовся з джерелом на {worst:.4} м"
    );
}

/// Патч, глибший за піраміду, бере висоту з предка — і на краю тайла це та
/// сама висота, що в предка.
///
/// Це те, на чому тримається відсутність тріщин у рельєфі: сусідні патчі
/// глибокого рівня можуть жити в **різних** тайлах предків, і на спільному
/// ребрі мусять дати те саме число.
#[test]
fn a_patch_deeper_than_the_pyramid_reads_its_ancestor() {
    let grid = grid();
    let terrain = build(&grid, LEVELS);

    // Пара сусідів на рівні, глибшому за піраміду, які лежать у різних
    // тайлах предків: `i = 1` і `i = 2` при `LEVELS = 3` — це діти різних
    // патчів рівня 2.
    let deep = LEVELS + 1;
    let left = Patch {
        face: 2,
        level: deep,
        i: (1 << deep) / 2 - 1,
        j: 3,
    };
    let right = Patch {
        face: 2,
        level: deep,
        i: (1 << deep) / 2,
        j: 3,
    };
    assert_ne!(
        terrain.covering(&left).0,
        terrain.covering(&right).0,
        "сусіди мали потрапити в різні тайли предків, інакше тест нічого не ловить"
    );

    let mut worst: f64 = 0.0;
    for b in 0..=SIDE {
        let a = terrain.height_m(&left, SIDE, b);
        let c = terrain.height_m(&right, 0, b);
        worst = worst.max((a - c).abs());
    }
    println!("  спільне ребро двох тайлів: розбіжність {worst:.6} м");
    assert_eq!(worst, 0.0, "рельєф розійшовся на межі тайлів");
}
