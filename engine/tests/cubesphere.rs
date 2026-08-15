//! Куб на сферу: три числа й одна рівність (ROADMAP-PLANETS.md, R1a).
//!
//! Нічого з цього не потребує ні GPU, ні вікна — тут сама арифметика, і саме
//! тому вона перевіряється до того, як щось намальовано. Тріщина на знімку —
//! одна темна лінія в піксель, яку око пропустить; рівність вершин її не
//! пропустить ніколи (правило 5 етапу R).

use engine::cubesphere::{grid, ratio, vertex, FACES};
use engine::sphere::EARTH_RADIUS_M;

const N: usize = 32;

/// Варп справді вирівнює сітку — і це видно лише поруч із наївною проєкцією.
///
/// Одне число тут не означало б нічого: «1.4» без «2.0» поруч не каже, добре
/// це чи погано.
#[test]
fn the_warp_makes_the_grid_more_even_than_plain_normalisation() {
    let naive = ratio(N, false, EARTH_RADIUS_M);
    let warped = ratio(N, true, EARTH_RADIUS_M);

    println!("  сітка {N}×{N}: наївна {naive:.4}, варпована {warped:.4}");

    assert!(
        warped < naive,
        "варп мав вирівняти сітку, а вийшло {warped:.4} проти {naive:.4}"
    );
    // Не «менше», а помітно менше: варп, що виграє третій знак, не вартий
    // тангенса при породженні.
    assert!(
        warped < 0.9 * naive,
        "виграш замалий, щоб платити за нього tan: {warped:.4} проти {naive:.4}"
    );
}

/// Кожна вершина лежить на сфері, а не поруч із нею.
#[test]
fn every_vertex_is_exactly_a_radius_from_the_centre() {
    let values = grid(N, true);
    let mut worst: f64 = 0.0;

    for face in 0..FACES {
        for &a in &values {
            for &b in &values {
                let p = vertex(face, a, b, EARTH_RADIUS_M);
                let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                worst = worst.max((r - EARTH_RADIUS_M).abs() / EARTH_RADIUS_M);
            }
        }
    }

    println!("  найбільше відхилення |r|: {worst:.2e} відносних");
    assert!(worst < 1e-15, "вершини не на сфері: {worst:.2e}");
}

/// Вершина на спільному ребрі двох граней — **той самий біт**.
///
/// Це головна перевірка кроку, і мутації під неї прогнані руками — з
/// результатом, який виправляє план (R1a):
///
/// - **не примусові кінці таблиці** (лишити `tan(π/4)` як є) — валить цей
///   тест. Саме той шов, заради якого крок існує;
/// - **дзеркало другим викликом `tan`** замість віднімання — цей тест
///   переживає, бо `tan` у glibc на цій машині виявився бітово непарним.
///   Ловить його сусідній тест таблиці, і в цьому вся його користь:
///   властивість, що тримається випадково, мусить мати сторожа, інакше
///   вона зникне на іншій платформі — а межа тут бітова;
/// - **перестановка осей `u` й `v` в одній грані** — те, що план називав
///   головною мутацією, — не валить **нічого**. І це не слабкість тесту:
///   транспонована грань дає ту саму **множину** вершин, тобто шва не
///   розриває взагалі. Розійдеться від неї не шов, а відповідність `(i, j)`
///   між сусідніми патчами — а патчів тут ще немає, тож і ловити її буде
///   R1b/R2b, де в індексів з'явиться зміст.
///
/// Перевіряються всі дванадцять ребер куба, а не одне.
#[test]
fn a_vertex_on_a_shared_edge_is_the_same_bits_from_both_faces() {
    let values = grid(N, true);
    let radius = EARTH_RADIUS_M;

    // Усі вершини кожної грані — за ключем із бітів позиції. Збіг ключа
    // означає бітову рівність усіх трьох компонент.
    let mut seen: std::collections::HashMap<[u64; 3], Vec<usize>> =
        std::collections::HashMap::new();
    for face in 0..FACES {
        for &a in &values {
            for &b in &values {
                let p = vertex(face, a, b, radius);
                let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
                let faces = seen.entry(key).or_default();
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
        }
    }

    // Скільки вершин ділять рівно дві грані (ребра) і скільки три (кути).
    let shared_by_two = seen.values().filter(|f| f.len() == 2).count();
    let shared_by_three = seen.values().filter(|f| f.len() == 3).count();

    println!("  спільних вершин: {shared_by_two} на ребрах, {shared_by_three} у кутах");

    // Дванадцять ребер по (N − 1) внутрішніх вершин: кути рахуються окремо.
    assert_eq!(
        shared_by_two,
        12 * (N - 1),
        "на ребрах збіглося не все — шов розходиться там, де його не видно"
    );
    // Вісім кутів куба, у кожному сходяться ТРИ грані, а не чотири: саме тут
    // ламається наївне зшивання (R2b про це ще нагадає).
    assert_eq!(shared_by_three, 8, "кути куба зійшлися не по три грані");

    // І жодна вершина не належить більш ніж трьом граням: чотири означало б,
    // що дві протилежні грані десь злилися.
    assert!(seen.values().all(|f| f.len() <= 3));
}

/// Таблиця параметрів симетрична бітово, а кінці — точні.
///
/// Окремою перевіркою, бо це передумова рівності вище, і зламати її можна,
/// не зачепивши нічого іншого: досить порахувати другу половину другим
/// викликом `tan` замість дзеркала.
#[test]
fn the_parameter_table_is_a_mirror_of_itself() {
    for n in [2, 8, 32, 33] {
        let values = grid(n, true);
        assert_eq!(values[0], -1.0, "лівий кінець не точний при n = {n}");
        assert_eq!(values[n], 1.0, "правий кінець не точний при n = {n}");

        for k in 0..=n {
            // Нуль — окремо, і не з педантизму: заперечення нуля міняє знак,
            // тож у точній середині бітова рівність із дзеркалом неможлива
            // за означенням. Натомість вимога до неї сильніша — рівно `+0.0`,
            // бо саме цим бітом вершини й порівнюються.
            if values[k] == 0.0 {
                assert_eq!(
                    values[k].to_bits(),
                    0.0_f64.to_bits(),
                    "середина при n = {n} — це «мінус нуль»"
                );
                continue;
            }
            assert_eq!(
                values[k].to_bits(),
                (-values[n - k]).to_bits(),
                "таблиця несиметрична в {k} при n = {n}"
            );
        }
    }

    // Парна сітка мусить мати точну нульову середину; непарна не має
    // середнього вузла взагалі.
    assert_eq!(grid(8, true)[4], 0.0);
}

// ---------------------------------------------------------------------------
// Патч: початок у f64, вершини у f32 (R1b)

use engine::camera::Camera;
use engine::cubesphere::{Patch, SIDE};

/// Похибка вершини — стала частка **розміру патча**, а не відстані.
///
/// Це правило 2 етапу R, і перевіряється воно як закон, а не як одне число:
/// на рівнях 0, 5 і 10 патч різниться в мільйон разів за розміром, а частка
/// мусить лишитися тією самою. Одне вимірювання на одному рівні пройшло б і
/// на реалізації, де зсув береться від чужого початку.
#[test]
fn a_vertex_is_off_by_a_fraction_of_the_patch_not_of_the_distance() {
    // `f32` дає 24 біти мантиси, тобто 6·10⁻⁸ відносних — те саме число, що
    // в reversed-Z (F3). Множник 2 — бо зсув від центра патча ще й
    // округлюється при відніманні.
    const TOLERANCE: f64 = 2.0 * 6e-8;

    for level in [0, 5, 10] {
        let patch = Patch {
            face: 4,
            level,
            i: (1 << level) / 3,
            j: (1 << level) / 2,
        };
        let mesh = patch.mesh(EARTH_RADIUS_M);

        // Розмір патча — довжина його діагоналі; з нею й порівнюється похибка.
        let corner = patch.vertex(0, 0, EARTH_RADIUS_M);
        let opposite = patch.vertex(SIDE, SIDE, EARTH_RADIUS_M);
        let size = ((corner[0] - opposite[0]).powi(2)
            + (corner[1] - opposite[1]).powi(2)
            + (corner[2] - opposite[2]).powi(2))
        .sqrt();

        let mut worst: f64 = 0.0;
        for a in 0..=SIDE {
            for b in 0..=SIDE {
                let exact = patch.vertex(a, b, EARTH_RADIUS_M);
                let offset = mesh.offsets[a * (SIDE + 1) + b];
                let rebuilt = [
                    mesh.origin[0] + f64::from(offset[0]),
                    mesh.origin[1] + f64::from(offset[1]),
                    mesh.origin[2] + f64::from(offset[2]),
                ];
                let error = ((rebuilt[0] - exact[0]).powi(2)
                    + (rebuilt[1] - exact[1]).powi(2)
                    + (rebuilt[2] - exact[2]).powi(2))
                .sqrt();
                worst = worst.max(error);
            }
        }

        println!(
            "  рівень {level:2}: патч {size:.3e} м, найгірша похибка {worst:.3e} м \
             = {:.2e} розміру",
            worst / size
        );
        assert!(
            worst <= TOLERANCE * size,
            "рівень {level}: {worst:.3e} м на патчі {size:.3e} м — це {:.2e} \
             розміру, а мало бути не більше {TOLERANCE:.0e}",
            worst / size
        );
    }
}

/// Камера за 10 м і камера за 4·10⁸ м бачать той самий патч.
///
/// Друга половина R1b, і без неї перша нічого не варта: похибка може бути
/// малою відносно патча й усе одно з'їдатися відніманням камери, якщо це
/// віднімання робиться не там, де треба. Тут воно робиться так, як його
/// робитиме GPU: `camera.relative(origin)` **один раз на патч**, плюс
/// повернутий зсув вершини — проти `camera.relative(exact)` **на кожну
/// вершину**, тобто проти того, що робив F4.
///
/// Міряється **кут**, а не метри, і це не оформлення результату. Абсолютна
/// розбіжність зобов'язана рости з відстанню: `f32` тримає сталу відносну
/// точність, тож на 4·10⁸ м його крок — 32 м, і обидва шляхи однаково
/// округляють до цієї сітки. Питання не в тому, чи зросли метри, а в тому, чи
/// зсунувся **силует**: розбіжність, поділена на відстань до самої вершини, —
/// це і є кут, на який вершина поїде на екрані.
///
/// Твердження, отже, сильне в правильній формі: не «похибка мала», а «кут той
/// самий на обох відстанях» — відстані немає в рівнянні.
#[test]
fn the_patch_looks_the_same_from_ten_metres_and_from_the_moon() {
    let patch = Patch {
        face: 0,
        level: 8,
        i: 100,
        j: 137,
    };
    let mesh = patch.mesh(EARTH_RADIUS_M);

    // Камера дивиться на центр патча з двох відстаней уздовж його нормалі.
    let direction = [
        mesh.origin[0] / EARTH_RADIUS_M,
        mesh.origin[1] / EARTH_RADIUS_M,
        mesh.origin[2] / EARTH_RADIUS_M,
    ];
    let mut angles = Vec::new();

    for distance in [10.0, 4.05e8] {
        let eye = [
            mesh.origin[0] + direction[0] * distance,
            mesh.origin[1] + direction[1] * distance,
            mesh.origin[2] + direction[2] * distance,
        ];
        let camera = Camera::look_at(eye, mesh.origin, [0.0, 0.0, 1.0]);

        // Патчевий шлях: камера віднімається від початку патча, зсув
        // додається вже у `f32`. Саме це робитиме вершинний шейдер.
        let base = camera.relative(mesh.origin);

        let mut worst_angle: f64 = 0.0;
        let mut worst_metres: f64 = 0.0;
        for a in 0..=SIDE {
            for b in 0..=SIDE {
                let offset = mesh.offsets[a * (SIDE + 1) + b];
                // Зсув живе у світових осях, тож у камерний простір його
                // повертає `rotate` — рівно те, що робитиме вершинний шейдер
                // матрицею вигляду.
                let turned = camera.rotate([
                    f64::from(offset[0]),
                    f64::from(offset[1]),
                    f64::from(offset[2]),
                ]);
                let by_patch = [
                    base[0] + turned[0],
                    base[1] + turned[1],
                    base[2] + turned[2],
                ];
                // Шлях F4: camera-relative на кожну вершину, з повного `f64`.
                let by_vertex = camera.relative(patch.vertex(a, b, EARTH_RADIUS_M));

                let gap = ((f64::from(by_patch[0]) - f64::from(by_vertex[0])).powi(2)
                    + (f64::from(by_patch[1]) - f64::from(by_vertex[1])).powi(2)
                    + (f64::from(by_patch[2]) - f64::from(by_vertex[2])).powi(2))
                .sqrt();
                // Відстань до самої вершини, а не до центра патча: зблизька
                // вони різняться на порядки, і саме дальні вершини дають
                // найбільші метри.
                let range = (f64::from(by_vertex[0]).powi(2)
                    + f64::from(by_vertex[1]).powi(2)
                    + f64::from(by_vertex[2]).powi(2))
                .sqrt();

                worst_metres = worst_metres.max(gap);
                worst_angle = worst_angle.max(gap / range);
            }
        }

        // Пікселі — щоб число мало зміст без перекладу: 1280 пікселів на
        // 60° поля зору, тобто радіан ≈ 1223 пікселі.
        println!(
            "  камера за {distance:.3e} м: {worst_metres:.3e} м, кут \
             {worst_angle:.2e} рад = {:.1e} пікселя",
            worst_angle * 1223.0
        );
        angles.push(worst_angle);
    }

    let (near, far) = (angles[0], angles[1]);
    assert!(
        near < 1e-6 && far < 1e-6,
        "силует їде: зблизька {near:.2e} рад, здалеку {far:.2e} рад"
    );
    // Головне твердження: відстань не входить у рівняння. Множник 10 —
    // запас на округлення самої камери, не на зростання з відстанню.
    assert!(
        far <= near.max(1e-12) * 10.0,
        "здалеку кут більший, ніж зблизька ({far:.2e} проти {near:.2e}) — \
         отже відстань таки входить у рівняння"
    );
}

/// Сусідні патчі ділять вершини **бітово**, і на межі рівнів теж.
///
/// Ось де перестановка осей у грані нарешті стає видимою (R1a про це прямо
/// каже): у патчів індекси мають зміст, і сусід за `i` мусить збігтися
/// краєм, а не «десь тією самою множиною».
#[test]
fn neighbouring_patches_share_their_edge_bit_for_bit() {
    let radius = EARTH_RADIUS_M;

    // Сусіди на одній грані.
    let left = Patch {
        face: 2,
        level: 3,
        i: 3,
        j: 5,
    };
    let right = Patch {
        face: 2,
        level: 3,
        i: 4,
        j: 5,
    };
    for b in 0..=SIDE {
        let a = left.vertex(SIDE, b, radius);
        let c = right.vertex(0, b, radius);
        for k in 0..3 {
            assert_eq!(
                a[k].to_bits(),
                c[k].to_bits(),
                "край між сусідами розійшовся у вузлі {b}, компонента {k}"
            );
        }
    }

    // Межа рівнів: патч рівня 3 і чотири патчі рівня 4 на його місці. Сітка
    // рівня 4 містить сітку рівня 3 у своїх парних вузлах — і це не
    // випадковість, а те, на чому триматиметься зшивання в R2b.
    let coarse = Patch {
        face: 2,
        level: 3,
        i: 3,
        j: 5,
    };
    let fine = Patch {
        face: 2,
        level: 4,
        i: 6,
        j: 10,
    };
    for a in 0..=SIDE / 2 {
        for b in 0..=SIDE / 2 {
            let from_coarse = coarse.vertex(a, b, radius);
            let from_fine = fine.vertex(2 * a, 2 * b, radius);
            for k in 0..3 {
                assert_eq!(
                    from_coarse[k].to_bits(),
                    from_fine[k].to_bits(),
                    "рівні 3 і 4 розійшлися у вузлі ({a}, {b}), компонента {k}"
                );
            }
        }
    }
}

/// Сітка патча замкнена: індекси в межах, трикутників рівно стільки, скільки
/// клітинок, і жодна вершина не загубилась.
#[test]
fn the_patch_mesh_is_closed() {
    let mesh = Patch {
        face: 5,
        level: 2,
        i: 1,
        j: 2,
    }
    .mesh(EARTH_RADIUS_M);
    let vertices = (SIDE + 1) * (SIDE + 1);

    assert_eq!(mesh.offsets.len(), vertices);
    assert_eq!(mesh.normals.len(), vertices);
    assert_eq!(mesh.indices.len(), SIDE * SIDE * 6);
    assert!(mesh.indices.iter().all(|&i| (i as usize) < vertices));

    let mut used = vec![false; vertices];
    for &i in &mesh.indices {
        used[i as usize] = true;
    }
    assert!(
        used.iter().all(|&u| u),
        "є вершини, яких не малює жоден трикутник"
    );

    // Нормаль — одиничний напрямок, а не позиція.
    for n in &mesh.normals {
        let length =
            (f64::from(n[0]).powi(2) + f64::from(n[1]).powi(2) + f64::from(n[2]).powi(2)).sqrt();
        assert!((length - 1.0).abs() < 1e-6, "нормаль довжиною {length}");
    }
}
