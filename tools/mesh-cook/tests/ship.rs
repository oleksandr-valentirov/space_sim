//! Кукер мешів проти чисел, які порахував Blender (ROADMAP, T5d2).
//!
//! Головна відмінність від оракула заглушки (V1): аналітичної таблиці для
//! імпортованої моделі не існує. Отже оракул береться **з іншого
//! інструмента** — те саме правило, що з етикеткою PDS3 у `dem-cook`.
//! Наш перерахунок тієї самої моделі перевіряв би сам себе.
//!
//! Оракулів три, і кожен ловить свій клас:
//!
//! 1. **знаковий об'єм** з `bmesh.calc_volume(signed=True)` — перевернутий
//!    обхід (знак), загублена оболонка, забутий масштаб;
//! 2. **габарити з JSON акесора** проти нашого читача `.bin` — порядок
//!    байтів, тип компонента, зсув `byteOffset`;
//! 3. **габарити в осях Blender** проти наших у осях glTF — конвенція осей,
//!    тобто те, що на симетричній моделі не видно взагалі.

use engine::mesh::Model;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    // `CARGO_MANIFEST_DIR` — це tools/mesh-cook.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn oracle() -> Value {
    let path = repository().join("assets-src/ship.oracle.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("оракул — це JSON")
}

fn number(value: &Value, key: &str) -> f64 {
    value[key].as_f64().unwrap_or_else(|| panic!("немає {key}"))
}

fn triple(value: &Value, key: &str) -> [f64; 3] {
    let list = value[key]
        .as_array()
        .unwrap_or_else(|| panic!("немає {key}"));
    [
        list[0].as_f64().unwrap(),
        list[1].as_f64().unwrap(),
        list[2].as_f64().unwrap(),
    ]
}

fn ship() -> mesh_cook::Cooked {
    mesh_cook::cook(&repository().join("assets-src/ship.gltf")).expect("модель мала прочитатись")
}

/// Об'єм нашого меша дорівнює об'єму з Blender.
///
/// Допуск відносний і виміряний, а не «на око»: координати в glTF — це `f32`,
/// тобто 10⁻⁷ відносних, і на конусі-зонді розбіжність вийшла 3.1·10⁻⁸
/// (скіл `blender-assets`). Допуск 10⁻⁶ лишає запас на порядок і все ще
/// ловить будь-яку помилку геометрії: перевернутий обхід міняє **знак**,
/// а загублена оболонка — відсотки.
#[test]
fn the_volume_is_the_one_blender_measured() {
    let cooked = ship();
    let expected = number(&oracle(), "volume_m3");
    let off = (cooked.volume_m3 - expected).abs() / expected.abs();
    println!("  об'єм: {} проти {expected} ({off:.2e})", cooked.volume_m3);
    assert!(
        off < 1e-6,
        "об'єм розійшовся: {} проти {expected}",
        cooked.volume_m3
    );
    // Знак окремо: він каже про обхід, і саме його ловить дзеркальна помилка.
    assert!(cooked.volume_m3 > 0.0, "обхід трикутників перевернутий");
}

/// Габарити в осях glTF — це габарити Blender з перестановкою `−Y → +Z`.
///
/// ⚠ Оракул тут навмисно записаний **в осях Blender**: щоб його відтворити,
/// читач мусить пройти через ту саму перестановку, яку робить експортер. На
/// симетричній моделі ця перевірка не означала б нічого — тому в моделі ніс,
/// стабілізатори, ілюмінатор і антена різні за всіма трьома осями.
#[test]
fn the_axes_arrive_the_way_the_convention_says() {
    let cooked = ship();
    let oracle = oracle();
    let low = triple(&oracle, "blender_min");
    let high = triple(&oracle, "blender_max");
    let height_m = cooked.model.height_m;

    // Меш уже нормалізований, тож наші габарити треба повернути в метри.
    let (mut ours_low, mut ours_high) = mesh_cook::bounds(&cooked.model.mesh);
    for k in 0..3 {
        ours_low[k] *= height_m;
        ours_high[k] *= height_m;
    }

    // glTF: x = x, y = z, z = −y. Отже межі по `z` беруться з `y` навпаки.
    let expected_low = [low[0], low[2], -high[1]];
    let expected_high = [high[0], high[2], -low[1]];
    println!("  наші {ours_low:?} … {ours_high:?}");
    println!("  чекали {expected_low:?} … {expected_high:?}");
    for k in 0..3 {
        assert!(
            (ours_low[k] - expected_low[k]).abs() < 1e-5,
            "нижня межа по осі {k}: {} проти {}",
            ours_low[k],
            expected_low[k]
        );
        assert!(
            (ours_high[k] - expected_high[k]).abs() < 1e-5,
            "верхня межа по осі {k}: {} проти {}",
            ours_high[k],
            expected_high[k]
        );
    }

    // Ніс дивиться в `+Z`, і це не наслідок габаритів: половина корпусу
    // попереду початку координат довша за половину позаду.
    assert!(
        ours_high[2] > 0.9 * height_m * 0.5,
        "ніс не в +Z: {ours_high:?}"
    );
}

/// Довжина й `extent` — ті самі числа, що порахував Blender.
///
/// `extent` не виводиться з довжини: у цієї моделі він 0.552 висоти, тобто
/// більший за половину — п'ята стабілізатора стоїть і нижче за сопло, і
/// збоку від нього. На ньому стоять `near` і камера третьої особи (V2), тож
/// помилка тут — це відсічений корпус, а не косметика.
#[test]
fn the_length_and_the_extent_are_blenders_numbers() {
    let cooked = ship();
    let oracle = oracle();
    let length = number(&oracle, "length_m");
    let extent = number(&oracle, "extent_m");

    println!(
        "  довжина {} проти {length}, extent {} проти {extent}",
        cooked.model.height_m,
        cooked.model.extent * cooked.model.height_m
    );
    assert!((cooked.model.height_m - length).abs() < 1e-5);
    assert!((cooked.model.extent * cooked.model.height_m - extent).abs() < 1e-5);
    assert!(
        cooked.model.extent > 0.52,
        "extent виявився половиною висоти: {}",
        cooked.model.extent
    );
}

/// Вершин у файлі більше, ніж у Blender, — і це нормально.
///
/// Кожен розрив нормалі розщеплює вершину, тож «скільки вершин у моделі» в
/// Blender не є числом, яке платить гра (скіл `blender-assets`). Перевірка
/// стереже саме це очікування: якби числа зрівнялися, це означало б, що
/// нормалі десь злилися й гладке затінення поїхало.
#[test]
fn the_file_carries_more_vertices_than_blender_shows() {
    let cooked = ship();
    let oracle = oracle();
    let in_blender = number(&oracle, "vertices_in_blender") as usize;
    let triangles = number(&oracle, "triangles") as usize;

    println!(
        "  вершин: {} у файлі проти {in_blender} у Blender",
        cooked.model.mesh.positions.len()
    );
    assert!(cooked.model.mesh.positions.len() > in_blender);
    assert_eq!(cooked.model.mesh.indices.len(), 3 * triangles);
}

/// Скукований файл читається назад і не залежить від прогону.
#[test]
fn cooking_twice_gives_the_same_file() {
    let first = ship().model.to_bytes();
    let second = ship().model.to_bytes();
    assert_eq!(first, second, "кукання не детерміноване");

    let read = Model::from_bytes(&first).expect("свій же файл");
    // Числа беруться з оракула, а не вписуються сюди: модель — джерело, яке
    // міняється (T9 перемалював її з референсу), і вписаний літерал зробив
    // би цей тест перевіркою пам'яті автора, а не круговороту байтів.
    let oracle = oracle();
    assert_eq!(
        read.mesh.indices.len(),
        3 * number(&oracle, "triangles") as usize
    );
    assert!((read.height_m - number(&oracle, "length_m")).abs() < 1e-5);
}

/// Фарба лягла **на ту саму геометрію**, а не поруч із нею (T9b).
///
/// Оракула-числа тут бути не може: `COLOR_0` — це той самий `.bin`, який ми
/// й читаємо, тож звірити його з собою означало б перевірити нічого. Тому
/// перевіряється **реєстрація**: кожен колір мусить знайтися рівно там, де
/// його поклала модель. Помилка кроку в акесорі або зсув на вершину лишає
/// кольори правильними за складом і перемішаними за місцем — оком це видно
/// як плями, а таким тестом як точне число.
///
/// Координати — в одиницях висоти від центра моделі: `along` у скрипті йде
/// від 0 до 1, а `+Z` у грі — це `along − 0.5`.
#[test]
fn the_paint_lands_where_the_model_put_it() {
    let cooked = ship();
    let paint = &cooked.model.paint;
    let points = &cooked.model.mesh.positions;
    assert_eq!(paint.len(), points.len(), "фарба не на кожну вершину");

    let mut palette: Vec<[u32; 3]> = paint.iter().map(|c| c.map(f32::to_bits)).collect();
    palette.sort_unstable();
    palette.dedup();
    println!("  палітра: {} кольорів", palette.len());
    // Шість — це рівно ті шість, що названі в `tools/blender/ship.py`: емаль,
    // червоне, жовте, сталь, шов і скло. Число, а не перелік значень: сюди
    // важливо, що фарба не розмазалась інтерполяцією й не злилась у одну.
    assert_eq!(palette.len(), 6, "палітра змінилася");

    let hot = |c: &[f32; 3], k: usize| c[k] > 0.5 && c[k] > 2.0 * c[(k + 2) % 3];
    let mut red = (0, 0);
    let mut yellow = 0;
    for (colour, point) in paint.iter().zip(points) {
        if hot(colour, 0) && colour[1] < 0.2 {
            // Червоне буває тільки двох сортів: носовий конус угорі й
            // стабілізатори внизу. Між ними його немає взагалі.
            assert!(
                point[2] > 0.36 || point[2] < -0.13,
                "червоне на середині корпусу: {point:?}"
            );
            if point[2] > 0.0 {
                red.0 += 1;
            } else {
                red.1 += 1;
            }
        }
        if hot(colour, 0) && colour[1] > 0.4 {
            // Жовте — тільки обідок ілюмінатора: правий борт, коло навколо
            // своєї точки. Радіус з моделі: 0.655 радіуса корпусу.
            yellow += 1;
            assert!(point[0] > 0.0, "жовте не на правому борті: {point:?}");
            let off = (point[2] - 0.136).hypot(point[1]);
            assert!(off < 0.09, "жовте поза ілюмінатором: {point:?}, {off}");
        }
    }
    println!("  червоних вершин: {} на носі, {} на хвості", red.0, red.1);
    println!("  жовтих вершин: {yellow}");
    assert!(
        red.0 > 0 && red.1 > 0,
        "червоне знайшлося лише з одного боку"
    );
    assert!(yellow > 0, "жовтого немає взагалі");
}

// ---------------------------------------------------------------------------
// Обидва типи індексів (T5d2)

/// Мінімальний glTF з одного трикутника — з індексами заданого типу.
///
/// Синтетичний, а не другий експорт з Blender: щоб отримати `UNSIGNED_INT`
/// природним шляхом, моделі треба понад 65 535 вершин, тобто мегабайти в git
/// заради двох рядків коду в читачі.
fn write_triangle(folder: &Path, component: u64) -> PathBuf {
    // Трикутник навскіс: усі три осі різні за розмахом, і `z` не нульова —
    // інакше нормалізувати до одиничної довжини нема на що.
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 4.0]];
    let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];

    let mut bin = Vec::new();
    for p in positions {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    for n in normals {
        for v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let indices_at = bin.len();
    for k in 0u32..3 {
        match component {
            5123 => bin.extend_from_slice(&(k as u16).to_le_bytes()),
            _ => bin.extend_from_slice(&k.to_le_bytes()),
        }
    }
    let index_bytes = bin.len() - indices_at;
    std::fs::write(folder.join("triangle.bin"), &bin).expect("запис .bin");

    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1},
            "indices": 2,
            "mode": 4
        }]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 2.0, 4.0]},
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
            {"bufferView": 2, "componentType": component, "count": 3, "type": "SCALAR"}
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 36},
            {"buffer": 0, "byteOffset": indices_at, "byteLength": index_bytes}
        ],
        "buffers": [{"uri": "triangle.bin", "byteLength": bin.len()}]
    });
    let path = folder.join("triangle.gltf");
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).expect("запис .gltf");
    path
}

/// Читач розрізняє `UNSIGNED_SHORT` і `UNSIGNED_INT`, а не припускає один.
///
/// Тип індексів вибирає експортер: до 65 535 вершин він дає `UNSIGNED_SHORT`
/// (саме це й лежить у нашій моделі), понад — `UNSIGNED_INT`. Читач, що знає
/// один тип, ламається тоді, коли міняли **форму**, а не код.
#[test]
fn both_index_types_read_the_same() {
    let mut meshes = Vec::new();
    for component in [5123u64, 5125] {
        let folder = std::env::temp_dir().join(format!("mesh-cook-{component}"));
        std::fs::create_dir_all(&folder).expect("тимчасовий каталог");
        let path = write_triangle(&folder, component);
        let cooked = mesh_cook::cook(&path).expect("трикутник мав прочитатись");
        assert_eq!(cooked.index_component, component);
        meshes.push(cooked.model.mesh.indices.clone());
        std::fs::remove_dir_all(&folder).ok();
    }
    assert_eq!(meshes[0], meshes[1], "типи індексів дали різні трикутники");
    assert_eq!(meshes[0], vec![0, 1, 2]);
}

/// Файл, у якого `.bin` розійшовся з JSON, — це помилка, а не тихий ассет.
#[test]
fn a_bin_that_disagrees_with_the_json_is_an_error() {
    let folder = std::env::temp_dir().join("mesh-cook-broken");
    std::fs::create_dir_all(&folder).expect("тимчасовий каталог");
    let path = write_triangle(&folder, 5123);

    // Псується саме `min` в акесорі — тобто те, що експортер опублікував.
    let text = std::fs::read_to_string(&path).unwrap();
    let mut json: Value = serde_json::from_str(&text).unwrap();
    json["accessors"][0]["max"][1] = serde_json::json!(7.0);
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    let message = mesh_cook::cook(&path).expect_err("розбіжність мала бути помилкою");
    println!("  {message}");
    assert!(
        message.contains("розійшлися"),
        "не те повідомлення: {message}"
    );
    std::fs::remove_dir_all(&folder).ok();
}
