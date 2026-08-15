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

/// **Ореол тайла — це справді сусідній вузол, і саме там, де його чекають**
/// (R7b).
///
/// Що саме ця перевірка пінить, і чого не пінить. Геометрію — «вузол ореолу
/// лежить на один крок за ребром» — доводить окремо й **незалежно від
/// формули** `engine::tests::cubesphere::a_halo_node_sits_one_step_past_the_edge`
/// (усередині грані бітово, через ребро куба відношенням кроків). Тут
/// доводиться друге: що кукер поклав це число **в ту комірку тайла**, з якої
/// його читатиме шейдер, і що воно бітово дорівнює тому, що сусід зберігає у
/// себе як звичайний вузол сітки.
///
/// Дві половини, і без другої перша нічого не варта:
///
/// 1. **Рівність із сусідом.** Копія й оригінал — те саме число. Розійтися
///    вони не мали б за побудовою (напрямок один), тож розбіжність означала б
///    зсув у розкладці, а не похибку.
/// 2. **Ореол не дорівнює краю.** Це та реалізація, проти якої крок і
///    робився: затиснений індекс дав би на межі тайла копію крайнього ряду,
///    пройшов би першу половину перевірки на ура — і дав би двом бокам межі
///    різні градієнти.
#[test]
fn the_halo_holds_the_neighbours_own_node() {
    use engine::cubesphere::{Edge, EDGES};

    let grid = grid();
    let terrain = build(&grid, LEVELS);

    let mut compared = 0;
    let mut same_as_edge = 0;
    for level in 0..LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let here = terrain.index(&patch).expect("рівень у піраміді");
                    for edge in EDGES {
                        for along in 0..=SIDE {
                            let (there, na, nb) = patch.halo_node(edge, along);
                            let theirs = terrain.node(
                                terrain.index(&there).expect("сусід у тій самій піраміді"),
                                na as i32,
                                nb as i32,
                            );

                            // Наша комірка ореолу й наш крайній вузол поруч.
                            let (side, k) = (SIDE as i32, along as i32);
                            let (ha, hb, ea, eb) = match edge {
                                Edge::AMin => (-1, k, 0, k),
                                Edge::AMax => (side + 1, k, side, k),
                                Edge::BMin => (k, -1, k, 0),
                                Edge::BMax => (k, side + 1, k, side),
                            };
                            let mine = terrain.node(here, ha, hb);
                            assert_eq!(
                                mine, theirs,
                                "{patch:?} / {edge:?}: ореол ({ha}, {hb}) дає {mine}, \
                                 а сусід {there:?} у вузлі ({na}, {nb}) — {theirs}"
                            );
                            if mine == terrain.node(here, ea, eb) {
                                same_as_edge += 1;
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
    }

    let flat = same_as_edge as f64 / compared as f64;
    println!(
        "  звірено {compared} вузлів ореолу; збігається з краєм {same_as_edge} \
         ({:.1}%)",
        flat * 100.0
    );
    assert!(
        flat < 0.5,
        "половина ореолу дорівнює крайньому ряду ({:.1}%) — це затиснений \
         індекс, а не сусід",
        flat * 100.0
    );
}

/// **Нахил на спільному ребрі — бітово одне число з обох боків** (R7c).
///
/// Це головна умова, під якою процедурній деталі взагалі можна дозволити
/// існувати. Амплітуда шуму йде від нахилу; якби нахил на спільному вузлі
/// різнився, деталь розірвала б поверхню рівно там, де R2b тріщину прибрав —
/// і виглядало б це не як помилка амплітуди, а як тріщина в геометрії.
///
/// Чому це може вийти бітово, а не «майже»: чотири значення центральної
/// різниці з обох боків — **ті самі числа**. Наш ореол `(−1, k)` є сусідів
/// вузол `(SIDE − 1, k)`, наш вузол `(1, k)` є його ореол, а `(0, k ± 1)`
/// лежать на самому ребрі й спільні. За ребром куба осі можуть помінятися
/// місцями й знаком — і саме тому амплітуда бере **довжину** градієнта:
/// додавання комутативне, квадрат знак з'їдає.
///
/// Перевіряються два випадки, і другий важливіший: патчі **глибші за
/// піраміду**, тобто ті, які й буде видно зблизька, коли деталь має сенс.
#[test]
fn the_slope_is_one_number_from_both_sides_of_an_edge() {
    use engine::cubesphere::{Edge, EDGES};

    let grid = grid();
    let terrain = build(&grid, LEVELS);

    // Вузол ребра з боку того, хто через нього дивиться.
    let node = |edge: Edge, k: usize| match edge {
        Edge::AMin => (0, k),
        Edge::AMax => (SIDE, k),
        Edge::BMin => (k, 0),
        Edge::BMax => (k, SIDE),
    };

    // Чи дістає стенсил центральної різниці до кута куба.
    //
    // Не «чи це сам кут»: стенсил тягнеться на `delta` вузлів **тайла**, тож
    // зіпсованим виявляється не вузол, а смуга навколо кута. У вузлах патча
    // її ширина — `delta · 2^deeper`.
    let tainted = |patch: &Patch, a: usize, b: usize| {
        let (tile, deeper) = terrain.covering(patch);
        let deepest = terrain.levels - 1;
        let delta = 2f64.powi(tile.level as i32 - deepest as i32);
        let reach = delta * f64::from(1u32 << deeper);

        let n = (SIDE << patch.level) as f64;
        let u = f64::from(patch.i * SIDE as u32 + a as u32);
        let v = f64::from(patch.j * SIDE as u32 + b as u32);
        u.min(n - u) <= reach && v.min(n - v) <= reach
    };

    let mut compared = 0;
    let mut across_faces = 0;
    let mut corners = 0;
    let mut worst_corner: f64 = 0.0;
    for level in [LEVELS - 1, LEVELS + 1] {
        let side = 1u32 << level;
        for face in 0..FACES {
            // Кути грані й одна клітинка всередині: там, де сходяться ребра
            // куба, помилка найімовірніша.
            for (i, j) in [(0, 0), (side - 1, side - 1), (0, side - 1), (1, 1)] {
                let patch = Patch { face, level, i, j };
                for edge in EDGES {
                    let there = patch.neighbour(edge);
                    if there.patch.face != face {
                        across_faces += 1;
                    }
                    for k in [0, 1, SIDE / 3, SIDE / 2, SIDE - 1, SIDE] {
                        let (ma, mb) = node(edge, k);
                        let (ta, tb) = node(there.edge, k);
                        let mine = terrain.slope_at(&patch, ma, mb);
                        let theirs = terrain.slope_at(&there.patch, ta, tb);

                        if tainted(&patch, ma, mb) {
                            // Смуга навколо кута куба: там стенсил однієї
                            // грані тягнеться в сусідню, а стенсил сусідньої —
                            // у третю, бо на куті сходяться ТРИ грані. Це межа
                            // конструкції, названа числом, а не похибка (Q3).
                            corners += 1;
                            worst_corner = worst_corner.max(
                                (mine - theirs).abs() / mine.max(theirs).max(f64::MIN_POSITIVE),
                            );
                            continue;
                        }

                        assert_eq!(
                            mine.to_bits(),
                            theirs.to_bits(),
                            "{patch:?} / {edge:?} вузол {k}: нахил {mine:.9e} проти \
                             {theirs:.9e} у {:?}",
                            there.patch
                        );
                        compared += 1;
                    }
                }
            }
        }
    }

    println!(
        "  {compared} вузлів ребра, з них через ребро куба {across_faces} \
         сусідств — нахил збігся бітово скрізь"
    );
    println!(
        "  вузлів у смузі навколо кутів пропущено {corners}; найгірша відносна \
         розбіжність там {:.1}%",
        worst_corner * 100.0
    );
    assert!(across_faces > 0, "жодного ребра куба серед перевірених");
    assert!(
        corners > 0,
        "жодного вузла в смузі навколо кута — виняток не перевірено"
    );
    // Смуга мусить лишатися смугою: якщо в неї потрапить помітна частка
    // вузлів, виняток перестане бути винятком.
    assert!(
        corners * 4 < compared,
        "{corners} зіпсованих вузлів проти {compared} чистих — це вже не смуга"
    );
}
