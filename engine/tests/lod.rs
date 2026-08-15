//! Вибір рівня патча тримається як механізм, а не як враження (R2a).
//!
//! Три твердження, і жодне з них не про красу: наближення камери ніколи не
//! знижує рівень, той самий кадр дає той самий набір, і два записані числа
//! кажуть, скільки все це коштує.
//!
//! GPU тут не потрібен: вибір рівня — геометрія на CPU, і саме тому він
//! перевіряється до того, як щось намалюється.

use engine::camera::Camera;
use engine::cubesphere::SIDE;
use engine::frame::FOV_Y;
use engine::lod::{self, Body};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const HEIGHT_PX: f64 = 720.0;

fn earth() -> Body {
    Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M)
}

/// Камера на висоті `altitude` над точкою, яку видно з грані `+X`.
fn above(altitude: f64) -> Camera {
    let d = EARTH_RADIUS_M + altitude;
    Camera::look_at([d, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
}

/// Наближення камери ніколи не знижує рівня жодної точки поверхні.
///
/// Сто положень, а не два: критерій — це відношення похибки до відстані, і
/// між двома точками воно монотонне майже завжди. Провал ловиться саме там,
/// де набір патчів **перебудовується**, і таких місць у діапазоні кілька.
///
/// Порівнюються не патчі, а **точки поверхні**: набори при різних висотах
/// складаються з різних патчів, тож поштучно їх зіставити нема з чим.
#[test]
fn coming_closer_never_lowers_the_level_of_a_point() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    // Від 4·10⁸ м (відстань до Місяця) до 100 км, логарифмічно.
    let far = 4.0e8_f64;
    let near = 1.0e5_f64;
    let steps = 100;

    // Три точки грані: центр, край і кут — вони переходять на новий рівень у
    // різні моменти, і кут найважчий, бо там сходяться три грані.
    let probes = [(0.5, 0.5), (0.5, 0.98), (0.02, 0.02)];
    let mut previous = [0u32; 3];
    let mut raised = 0;

    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let altitude = far * (near / far).powf(t);
        let selection = lod::select(&earth(), &above(altitude), focal);

        for (index, &(u, v)) in probes.iter().enumerate() {
            let level = lod::level_at(&selection, 0, u, v)
                .unwrap_or_else(|| panic!("точка ({u}, {v}) не накрита жодним патчем"));
            assert!(
                level >= previous[index],
                "на висоті {altitude:.3e} м точка ({u}, {v}) впала з рівня {} на {level}",
                previous[index]
            );
            if level > previous[index] {
                raised += 1;
            }
            previous[index] = level;
        }
    }

    // Без цього тест був би зелений і на критерії, який завжди віддає нуль.
    println!("  рівень зростав {raised} разів на 101 положенні");
    assert!(
        raised >= 6,
        "рівень зріс лише {raised} разів — критерій ні на що не реагує"
    );
}

/// Той самий кадр дає той самий набір — і в тому ж порядку.
///
/// Порядок тут не педантизм: набір їде у буфер GPU, і переставлений щокадру
/// список означав би повне перезавантаження буфера там, де насправді нічого
/// не змінилось.
#[test]
fn the_same_camera_gives_the_same_patches() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    for altitude in [3.0e5, 2.0e6, 4.0e8] {
        let first = lod::select(&earth(), &above(altitude), focal);
        let second = lod::select(&earth(), &above(altitude), focal);
        assert_eq!(
            first.patches, second.patches,
            "на висоті {altitude:.1e} м два виклики дали різні набори"
        );
    }
}

/// Два числа, які борг проти реальності: скільки патчів на низькій орбіті й
/// скільки з відстані до Місяця.
///
/// Верхня межа тут — не оракул точності, а сторож: критерій без стелі здатен
/// поділити планету до мільйона патчів і зробити це тихо.
#[test]
fn the_count_stays_where_it_can_be_afforded() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

    for (name, altitude) in [("низька орбіта", 3.0e5), ("відстань до Місяця", 4.0e8)]
    {
        let selection = lod::select(&earth(), &above(altitude), focal);
        let levels: Vec<u32> = {
            let mut l: Vec<u32> = selection.patches.iter().map(|p| p.level).collect();
            l.sort_unstable();
            l.dedup();
            l
        };
        println!(
            "  {name} ({altitude:.1e} м): {} патчів, рівні {levels:?}, {} вершин, стеля зачепила {}",
            selection.patches.len(),
            lod::vertex_count(&selection),
            selection.clamped
        );

        assert_eq!(
            selection.clamped, 0,
            "{name}: стеля рівня спрацювала там, де не мала б"
        );
        assert!(
            selection.patches.len() <= 4096,
            "{name}: {} патчів — це вже не кадр, а перебір",
            selection.patches.len()
        );
        // Кожен патч — це (SIDE + 1)² вершин, скільки б їх не було.
        assert_eq!(
            lod::vertex_count(&selection),
            selection.patches.len() * (SIDE + 1) * (SIDE + 1)
        );
    }
}

/// Критерій знає про роздільність — і це те, чого відстань до камери не вміє.
///
/// Головна підміна, яка може прокрастися сюди непомітно: вибір, що дивиться
/// лише на відстань, проходить усі перевірки вище — і монотонність, і
/// детермінізм, і кількість. Валить його рівно це: з тієї самої точки у
/// вищому кадрі патчів мусить стати більше, бо піксель став дрібнішим.
///
/// **Набір міняється через одне подвоєння, і це арифметика, а не вада.**
/// Рівень коштує вчетверо меншу похибку, подвоєна роздільність купує вдвічі
/// більшу — тож прийнятий рівень щоразу сідає на ~0.5 px, наступне подвоєння
/// доводить його до ~1.0 px і нічого не міняє, а через одне вже ділить.
/// Виміряно: 9 патчів на 720, 21 на 1440 **і на 2880**, 45 на 5760. Тому
/// строга нерівність вимагається на всьому діапазоні, а не між сусідами.
#[test]
fn a_taller_frame_needs_finer_patches_from_the_same_point() {
    let mut counts = Vec::new();
    for height in [720.0, 1440.0, 2880.0, 5760.0] {
        let selection = lod::select(&earth(), &above(3.0e5), lod::focal_px(FOV_Y, height));
        println!("  {height} px заввишки: {} патчів", selection.patches.len());
        counts.push(selection.patches.len());
    }

    for pair in counts.windows(2) {
        assert!(
            pair[1] >= pair[0],
            "вищий кадр дав менше патчів: {} проти {}",
            pair[1],
            pair[0]
        );
    }
    assert!(
        counts[3] > counts[0],
        "учетверо вищий кадр дав {} патчів проти {} — критерій не бачить \
         роздільності",
        counts[3],
        counts[0]
    );
}

/// З відстані до Місяця планета не дрібніша за шість граней.
///
/// Це нижній бік критерію: він мусить не лише додавати патчі зблизька, а й
/// **не додавати** їх здалеку. Шість патчів — рівно грані куба, тобто вибір
/// не поділив нічого.
#[test]
fn from_far_away_the_planet_is_six_faces() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let selection = lod::select(&earth(), &above(4.0e8), focal);
    assert_eq!(
        selection.patches.len(),
        6,
        "здалеку вибрано {} патчів замість шести граней",
        selection.patches.len()
    );
}

/// **Обіцянка вибору, перевірена там, де камера стоїть не над центром грані**
/// (D13).
///
/// Усі перевірки вище ставлять камеру в `[d, 0, 0]` — рівно над центром грані
/// `+X`. Це найзручніша точка з усіх можливих, і саме тому вада прожила там
/// непоміченою: `error_px` міряла відстань до **чотирьох кутів і центру**
/// патча, а над центром грані центральний вузол і **є** найближчою точкою,
/// тобто відповідь виходила точною випадково. Варто зсунути камеру на двадцять
/// градусів — і найближчий вузол опиняється вчетверо далі за поверхню під
/// ногами; у грі це давало **шість патчів рівня 0 на будь-якій висоті**, аж до
/// зникнення Місяця з кадру нижче за 165 км.
///
/// Тому оракул тут не «скільки патчів», а сама обіцянка: **похибка кожного
/// патча, поділена на справжню відстань до нього, менша за
/// [`lod::TOLERANCE_PX`]**. Справжню — тобто пораховану перебором усіх
/// `(SIDE+1)²` вузлів патча, а не тим самим наближенням, яке перевіряється.
/// Обхід дорогий; він і має бути дорогим — це еталон, а не кадр.
///
/// Напрямки взяті золотою спіраллю: вони не збігаються ні з віссю грані, ні з
/// її кутом, ні між собою — тобто рівно те, чого фікстура `above` не давала.
#[test]
fn the_tolerance_holds_wherever_the_camera_sits() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    // Найгірше з поміченого — Місяць: він менший, тож і похибка грані менша, і
    // до її зникнення критерій добирався тихіше.
    const MOON_RADIUS_M: f64 = 1_737_530.0;

    let mut worst: f64 = 0.0;
    let mut checked = 0;

    for (name, radius) in [("Земля", EARTH_RADIUS_M), ("Місяць", MOON_RADIUS_M)] {
        for altitude in [5.0e3_f64, 2.0e4, 1.0e5, 1.7e5, 3.0e5, 2.0e6] {
            let body = Body::still([0.0, 0.0, 0.0], radius);
            let mut peak: f64 = 0.0;
            let mut patches = 0;

            // Тридцять два напрямки золотою спіраллю — рівномірно по сфері й
            // без жодної симетрії з гранями куба.
            for k in 0..32 {
                let z = 1.0 - (2.0 * f64::from(k) + 1.0) / 32.0;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let phi = f64::from(k) * std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
                let u = [r * phi.cos(), r * phi.sin(), z];

                let d = radius + altitude;
                let eye = [u[0] * d, u[1] * d, u[2] * d];
                let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
                let selection = lod::select(&body, &camera, focal);
                patches = patches.max(selection.patches.len());

                for patch in &selection.patches {
                    if patch.level >= lod::MAX_LEVEL {
                        // Стеля ріже якість свідомо, і `Selection::clamped`
                        // про це вже сказав.
                        continue;
                    }
                    // Еталонна відстань: перебір усіх вузлів патча. Дорожче за
                    // будь-яке наближення рівно настільки, наскільки треба,
                    // щоб не повторити наближення, яке перевіряється.
                    let mut nearest = f64::INFINITY;
                    for a in 0..=SIDE {
                        for b in 0..=SIDE {
                            let p = patch.vertex(a, b, radius);
                            let v = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
                            nearest =
                                nearest.min((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
                        }
                    }
                    let px = lod::error_m(patch, radius) / nearest.max(1.0) * focal;
                    peak = peak.max(px);
                    checked += 1;
                    assert!(
                        px <= lod::TOLERANCE_PX,
                        "{name}, висота {altitude:.1e} м, напрямок {u:?}: {patch:?} \
                         дає {px:.2} px похибки при допуску {:.1}",
                        lod::TOLERANCE_PX
                    );
                }
            }

            println!(
                "  {name}, {altitude:.1e} м: найгірша похибка {peak:.3} px, \
                 найбільший набір {patches} патчів"
            );
            worst = worst.max(peak);
        }
    }

    // Сторож проти перевірки, яка нічого не перевіряє: якщо найгірший випадок
    // на всьому діапазоні лишився далеко під допуском, значить критерій ділить
    // із запасом і межу тут ніхто не мацав.
    assert!(
        worst > lod::TOLERANCE_PX / 4.0,
        "найгірша похибка на всьому діапазоні {worst:.3} px — перевірка не \
         доходить до допуску"
    );
    println!("  перевірено {checked} патчів, найгірший {worst:.3} px");
}

// ---------------------------------------------------------------------------
// Зшивання рівнів (R2b)

use engine::cubesphere::{self, Patch, EDGES};
use std::collections::HashMap;

/// Сусіди в наборі різняться не більш ніж на рівень — і вирівнювання таки
/// довелося робити.
///
/// Друга половина обов'язкова: набір, у якому всі патчі одного рівня,
/// проходить першу перевірку й нічого не доводить. Тому поруч стоїть число
/// `balanced` — скільки патчів довелося додати понад те, що просив критерій
/// похибки. Нуль на всіх висотах означав би, що правило перевіряється на
/// матеріалі, який його не порушує.
#[test]
fn no_neighbour_in_the_set_is_two_levels_away() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let mut added = 0;

    for altitude in [1.0e5, 3.0e5, 1.0e6, 1.0e7, 4.0e8] {
        let selection = lod::select(&earth(), &above(altitude), focal);
        let leaves: std::collections::HashSet<Patch> = selection.patches.iter().copied().collect();

        for patch in &selection.patches {
            for edge in EDGES {
                let mut cell = patch.neighbour(edge).patch;
                // Лист, що накриває сусідню клітинку, — вона сама або предок.
                let level = loop {
                    if leaves.contains(&cell) {
                        break Some(cell.level);
                    }
                    match cell.parent() {
                        Some(up) => cell = up,
                        None => break None,
                    }
                };
                // `None` — на тому боці набір дрібніший; тоді різницю міряє
                // той бік, і міряє її так само.
                if let Some(level) = level {
                    assert!(
                        patch.level - level <= 1,
                        "на висоті {altitude:.1e} м {patch:?} сусідить через \
                         {edge:?} з рівнем {level}"
                    );
                }
            }
        }

        println!(
            "  {altitude:.1e} м: {} патчів, з них {} додало вирівнювання",
            selection.patches.len(),
            selection.balanced
        );
        added += selection.balanced;
    }

    assert!(
        added > 0,
        "вирівнювання не додало жодного патча на жодній висоті — правило \
         перевірене на матеріалі, який його не порушує"
    );
}

/// **Головна перевірка кроку: поверхня замкнена.**
///
/// Тріщина — це діра, а діра — це ребро трикутника, у якого немає пари. На
/// замкненій поверхні кожне неорієнтоване ребро належить рівно двом
/// трикутникам, і це твердження не знає ні про рівні, ні про грані, ні про
/// маски: воно ловить і незшитий стик рівнів, і переплутану грань, і кут
/// куба, де сходяться **три** патчі замість чотирьох.
///
/// Вершини порівнюються **бітами**, не з допуском. Допуск тут означав би, що
/// тріщина в мікрометр — не тріщина, а вона саме тріщина: розрив у пікселі
/// з'являється не від розміру щілини, а від того, що фон видно наскрізь.
///
/// Вироджені трикутники (двоє з трьох вузлів збіглися) відкидаються: саме
/// ними зшивання й прибирає непарний вузол, растеризатор їх не малює, і в
/// підрахунку ребер вони були б шумом.
#[test]
fn the_stitched_surface_has_no_edge_without_a_pair() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

    for altitude in [1.0e5, 3.0e5, 2.0e6] {
        let selection = lod::select(&earth(), &above(altitude), focal);

        // Позиція → номер. Бітова рівність стає рівністю номерів, і далі
        // ребра рахуються цілими числами.
        let mut ids: HashMap<[u64; 3], u32> = HashMap::new();
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        let mut used: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut degenerate = 0;
        let mut triangles = 0;

        for (patch, &mask) in selection.patches.iter().zip(&selection.masks) {
            // Одинична сфера: множник радіуса нічого не додає до топології.
            let nodes: Vec<u32> = {
                let mut v = Vec::with_capacity((SIDE + 1) * (SIDE + 1));
                for a in 0..=SIDE {
                    for b in 0..=SIDE {
                        let p = patch.vertex(a, b, 1.0);
                        let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
                        let next = ids.len() as u32;
                        v.push(*ids.entry(key).or_insert(next));
                    }
                }
                v
            };

            for tri in cubesphere::indices(mask).chunks(3) {
                let t = [
                    nodes[tri[0] as usize],
                    nodes[tri[1] as usize],
                    nodes[tri[2] as usize],
                ];
                if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
                    degenerate += 1;
                    continue;
                }
                triangles += 1;
                used.extend(t);
                for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *edges.entry((x.min(y), x.max(y))).or_default() += 1;
                }
            }
        }

        let lonely: Vec<_> = edges.iter().filter(|(_, &n)| n != 2).collect();
        println!(
            "  {altitude:.1e} м: {} патчів, {triangles} трикутників, \
             {degenerate} вироджених, {} ребер, {} без пари",
            selection.patches.len(),
            edges.len(),
            lonely.len()
        );
        assert!(
            lonely.is_empty(),
            "на висоті {altitude:.1e} м {} ребер належать не двом трикутникам \
             — це тріщина",
            lonely.len()
        );

        // Ейлерова характеристика сфери: V − E + F = 2. Замкненість без неї
        // була б і в поверхні, склеєної сама з собою навиворіт.
        //
        // Вершини рахуються **вжиті**, а не всі: непарний вузол зшитого ребра
        // лишається в сітці (індекси адресують її цілком), але не належить
        // жодному трикутнику — і в топології його немає. Різниця тут не
        // косметична: саме вона дорівнює кількості вироджених трикутників, і
        // саме з неї видно, що зшивання прибрало рівно те, що збиралось.
        assert_eq!(
            ids.len() - used.len(),
            degenerate,
            "викинутих вузлів і вироджених трикутників має бути порівну"
        );
        let v = used.len() as i64;
        let e = edges.len() as i64;
        let f = triangles as i64;
        assert_eq!(
            v - e + f,
            2,
            "на висоті {altitude:.1e} м V − E + F = {}, а сфера дає 2",
            v - e + f
        );
    }
}
