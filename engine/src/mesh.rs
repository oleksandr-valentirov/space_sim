//! Формат скукованого меша (етап T, крок T5d2).
//!
//! Той самий вибір, що з тайлами (`crate::tiles`): формат живе **в рушії**,
//! бо записувач і читач одного формату мусять бути одним кодом. Кукер
//! (`tools/mesh-cook`) залежить від рушія, рушій про кукер не знає нічого.
//!
//! ## Меш одиничної висоти, а метри — поруч
//!
//! Рушій тримає корабель одиничної висоти й масштабує його `height_m`
//! щокадру (V2), тож нормалізацію робить кукер, один раз. У файлі лежать два
//! числа поруч із геометрією:
//!
//! - `height_m` — довжина оригіналу вздовж `+Z` у метрах. Довідка про
//!   модель: гра вільна масштабувати корабель як завгодно;
//! - `extent` — радіус обмежувальної сфери **в одиницях висоти**. Не
//!   виводиться з першого й не є половиною одиниці: стабілізатори виступають
//!   за корпус, а справжня модель виступає як завгодно. На ньому стоять
//!   `near` і камера третьої особи (V2), тож помилка в ньому — це відсічений
//!   корпус.
//!
//! ## Позиції у `f32`, і це не те саме рішення, що в патчі
//!
//! У планети координати великі, і саме тому вершини там camera-relative. Тут
//! меш нормалізований до одиниці, тобто найбільше число у файлі — одиниці, а
//! `f32` дає на них 10⁻⁷ відносних. Корабель заввишки 6 м це 0.6 мкм; на
//! екрані такого не буває.
//!
//! ## Осі не перетворюються ніде
//!
//! Модель робиться носом уздовж `−Y` у Blender, експорт дефолтами дає glTF з
//! носом уздовж `+Z` — а це вже конвенція `Scene::Ship`. Отже кукер
//! **перекладає осі рівно один раз**, з glTF у наші (`y` вгору проти `z`
//! вгору не міняється: обидва тут праворукі з носом по `+Z`), і в цьому
//! форматі ніяких осей уже немає — лише числа.

use crate::sphere::Mesh;

/// Підпис файлу. Вісім байтів, щоб заголовок читався оком у hex-дампі.
pub const MAGIC: [u8; 8] = *b"SSMSH\0\0\0";

/// Версія формату. Росте при будь-якій зміні розкладки.
///
/// Версія 2 (T9b) додала фарбу: три `f32` на вершину після нормалей, і слово
/// у заголовку про те, є вона взагалі чи ні.
pub const VERSION: u32 = 2;

/// Підпис, версія, дві кількості, прапорець фарби й два числа моделі.
const HEADER_BYTES: usize = 8 + 4 + 4 + 4 + 4 + 4 + 4;

/// Меш моделі разом з тим, що про неї треба знати.
#[derive(Clone, Debug)]
pub struct Model {
    /// Довжина оригіналу вздовж `+Z`, метри.
    pub height_m: f64,
    /// Радіус обмежувальної сфери в одиницях висоти.
    pub extent: f64,
    /// Геометрія, нормалізована до одиничної висоти.
    pub mesh: Mesh,
    /// Базовий колір на вершину, **лінійне світло**; порожньо — фарби немає.
    ///
    /// На вершину, а не діапазонами за матеріалами: у glTF це `COLOR_0`, і
    /// модель лишається **одним примітивом**, тобто одним викликом малювання
    /// і жодного нового поняття у форматі. Ціна відома й заплачена — розриви
    /// кольору розщеплюють вершини так само, як розриви нормалі.
    ///
    /// ⚠ Тільки колір. Шорсткість і метал лишаються сталими на корабель:
    /// `COLOR_0` їх не везе, а другий шлях для них — це вже діапазони за
    /// матеріалами, тобто рішення формату, і воно чекає на першу деталь, якій
    /// цього справді бракує (скло ілюмінатора поки що обходиться кольором).
    pub paint: Vec<[f32; 3]>,
}

impl Model {
    /// Нормалізувати меш у метрах до одиничної висоти.
    ///
    /// Один код на кукер і на будь-якого іншого викликача: два способи
    /// поділити на довжину дали б два різні кораблі.
    pub fn from_metres(mesh: Mesh, paint: Vec<[f32; 3]>) -> Result<Model, String> {
        if mesh.positions.is_empty() {
            return Err("меш без вершин".to_string());
        }
        if !paint.is_empty() && paint.len() != mesh.positions.len() {
            return Err(format!(
                "фарби на {} вершин при {} вершинах геометрії",
                paint.len(),
                mesh.positions.len()
            ));
        }
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for p in &mesh.positions {
            low = low.min(p[2]);
            high = high.max(p[2]);
        }
        let height_m = high - low;
        if height_m <= 0.0 || !height_m.is_finite() {
            return Err(format!("модель нульової довжини вздовж +Z: {height_m}"));
        }

        // Початок координат лишається там, де його поставила модель: у V2 він
        // на середині корпусу, і камера третьої особи цілиться саме туди.
        // Центрувати тут означало б посунути корабель відносно того, чим його
        // веде гра.
        let mut mesh = mesh;
        for p in &mut mesh.positions {
            for v in p.iter_mut() {
                *v /= height_m;
            }
        }
        let extent = mesh
            .positions
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0.0, f64::max);

        Ok(Model {
            height_m,
            extent,
            mesh,
            paint,
        })
    }

    /// Байти файлу.
    pub fn to_bytes(&self) -> Vec<u8> {
        let vertices = self.mesh.positions.len();
        let stride = if self.paint.is_empty() { 24 } else { 36 };
        let mut out =
            Vec::with_capacity(HEADER_BYTES + vertices * stride + self.mesh.indices.len() * 4);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(vertices as u32).to_le_bytes());
        out.extend_from_slice(&(self.mesh.indices.len() as u32).to_le_bytes());
        out.extend_from_slice(&u32::from(!self.paint.is_empty()).to_le_bytes());
        out.extend_from_slice(&(self.height_m as f32).to_le_bytes());
        out.extend_from_slice(&(self.extent as f32).to_le_bytes());
        for p in &self.mesh.positions {
            for v in p {
                out.extend_from_slice(&(*v as f32).to_le_bytes());
            }
        }
        for n in &self.mesh.normals {
            for v in n {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        for c in &self.paint {
            for v in c {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        for i in &self.mesh.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
        out
    }

    /// Прочитати файл. Помилка — це рядок, а не паніка: асета може не бути на
    /// диску, і сказати про це мусить викликач.
    pub fn from_bytes(bytes: &[u8]) -> Result<Model, String> {
        if bytes.len() < HEADER_BYTES {
            return Err(format!(
                "файл коротший за заголовок: {} байтів",
                bytes.len()
            ));
        }
        if bytes[0..8] != MAGIC {
            return Err("не той підпис: це не меш".to_string());
        }
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let float = |at: usize| {
            f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let version = word(8);
        if version != VERSION {
            return Err(format!("версія {version}, а читач розуміє {VERSION}"));
        }
        let vertices = word(12) as usize;
        let indices = word(16) as usize;
        let painted = word(20) == 1;
        let height_m = f64::from(float(24));
        let extent = f64::from(float(28));

        let stride = if painted { 36 } else { 24 };
        let need = HEADER_BYTES + vertices * stride + indices * 4;
        if bytes.len() != need {
            return Err(format!(
                "файл на {} байтів, а {vertices} вершин і {indices} індексів вимагають {need}",
                bytes.len()
            ));
        }

        let mut at = HEADER_BYTES;
        let mut positions = Vec::with_capacity(vertices);
        for _ in 0..vertices {
            let p = [float(at), float(at + 4), float(at + 8)];
            positions.push([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
            at += 12;
        }
        let mut normals = Vec::with_capacity(vertices);
        for _ in 0..vertices {
            normals.push([float(at), float(at + 4), float(at + 8)]);
            at += 12;
        }
        let mut paint = Vec::new();
        if painted {
            paint.reserve(vertices);
            for _ in 0..vertices {
                paint.push([float(at), float(at + 4), float(at + 8)]);
                at += 12;
            }
        }
        let mut list = Vec::with_capacity(indices);
        for _ in 0..indices {
            let index = word(at);
            if index as usize >= vertices {
                return Err(format!("індекс {index} при {vertices} вершинах"));
            }
            list.push(index);
            at += 4;
        }

        Ok(Model {
            height_m,
            extent,
            paint,
            mesh: Mesh {
                positions,
                normals,
                indices: list,
            },
        })
    }
}

/// Знаковий об'єм замкненої оболонки — оракул геометрії.
///
/// Сума `(a × b) · c / 6` по трикутниках: для замкненої оболонки з нормалями
/// назовні вона додатна й дорівнює об'єму, а перевернутий обхід міняє знак.
/// Незамкнена дає число без змісту — і саме тому оракул звіряється з
/// **іншим інструментом** (`bmesh.calc_volume`), а не сам із собою.
pub fn signed_volume(mesh: &Mesh) -> f64 {
    let mut total = 0.0;
    for triangle in mesh.indices.chunks_exact(3) {
        let a = mesh.positions[triangle[0] as usize];
        let b = mesh.positions[triangle[1] as usize];
        let c = mesh.positions[triangle[2] as usize];
        total += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Куб зі стороною `side`, вісім вершин, нормалі — назовні по осях.
    fn cube(side: f64) -> Mesh {
        let h = 0.5 * side;
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        for z in [-h, h] {
            for y in [-h, h] {
                for x in [-h, h] {
                    positions.push([x, y, z]);
                    let n = (x * x + y * y + z * z).sqrt();
                    normals.push([(x / n) as f32, (y / n) as f32, (z / n) as f32]);
                }
            }
        }
        // Вершини за бітами (x, y, z); грані виписані з обходом назовні.
        let quad = |a: u32, b: u32, c: u32, d: u32| vec![a, b, c, a, c, d];
        let mut indices = Vec::new();
        for face in [
            quad(0, 2, 3, 1), // −z
            quad(4, 5, 7, 6), // +z
            quad(0, 1, 5, 4), // −y
            quad(2, 6, 7, 3), // +y
            quad(0, 4, 6, 2), // −x
            quad(1, 3, 7, 5), // +x
        ] {
            indices.extend(face);
        }
        Mesh {
            positions,
            normals,
            indices,
        }
    }

    /// Об'єм куба — це об'єм куба, і знак каже про обхід.
    ///
    /// Оракул самого оракула: далі ним звіряється модель проти Blender, тож
    /// помилка тут проїхала б непоміченою в обидва боки.
    #[test]
    fn the_signed_volume_of_a_cube_is_its_volume() {
        let mesh = cube(2.0);
        assert!((signed_volume(&mesh) - 8.0).abs() < 1e-12);

        // Перевернутий обхід — той самий об'єм з мінусом.
        let mut flipped = mesh.clone();
        for triangle in flipped.indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
        assert!((signed_volume(&flipped) + 8.0).abs() < 1e-12);
    }

    /// Нормалізація ділить на довжину вздовж `+Z` і більше ні на що.
    #[test]
    fn a_model_comes_out_one_unit_long() {
        let model = Model::from_metres(cube(6.0), Vec::new()).expect("куб — це модель");
        assert!((model.height_m - 6.0).abs() < 1e-12);
        let low = model
            .mesh
            .positions
            .iter()
            .fold(f64::INFINITY, |acc, p| acc.min(p[2]));
        let high = model
            .mesh
            .positions
            .iter()
            .fold(f64::NEG_INFINITY, |acc, p| acc.max(p[2]));
        assert!((high - low - 1.0).abs() < 1e-12, "довжина {}", high - low);

        // Радіус обмежувальної сфери куба — половина його діагоналі.
        assert!(
            (model.extent - 0.75_f64.sqrt()).abs() < 1e-12,
            "extent {}",
            model.extent
        );
        // І він більший за половину висоти — тобто не виводиться з неї.
        assert!(model.extent > 0.5);
    }

    /// Файл повертає рівно те, що в нього поклали.
    #[test]
    fn a_model_survives_the_round_trip() {
        let model = Model::from_metres(cube(6.0), Vec::new()).expect("куб — це модель");
        let read = Model::from_bytes(&model.to_bytes()).expect("свій же файл має читатися");

        assert_eq!(read.height_m as f32, model.height_m as f32);
        assert_eq!(read.extent as f32, model.extent as f32);
        assert_eq!(read.mesh.indices, model.mesh.indices);
        assert_eq!(read.mesh.normals, model.mesh.normals);
        for (a, b) in read.mesh.positions.iter().zip(&model.mesh.positions) {
            for k in 0..3 {
                assert_eq!(a[k] as f32, b[k] as f32, "вершина поїхала");
            }
        }
    }

    /// Фарбована модель теж повертає рівно те, що в неї поклали, — і
    /// нефарбована лишається нефарбованою.
    ///
    /// Дві половини одного питання: у файлі про фарбу є **слово в заголовку**,
    /// і саме воно вирішує, скільки байтів на вершину читати далі. Помилка тут
    /// не дала б помилки читання — вона зсунула б індекси, тобто віддала б
    /// правдоподібний меш з переплутаною геометрією.
    #[test]
    fn paint_survives_the_round_trip() {
        let mesh = cube(6.0);
        let paint: Vec<[f32; 3]> = (0..mesh.positions.len())
            .map(|k| [k as f32 / 16.0, 0.25, 0.5])
            .collect();
        let model = Model::from_metres(mesh, paint.clone()).expect("куб — це модель");
        let read = Model::from_bytes(&model.to_bytes()).expect("свій же файл має читатися");
        assert_eq!(read.paint, paint);
        assert_eq!(read.mesh.indices, model.mesh.indices);
        assert_eq!(read.mesh.normals, model.mesh.normals);

        let plain = Model::from_metres(cube(6.0), Vec::new()).expect("куб — це модель");
        let read = Model::from_bytes(&plain.to_bytes()).expect("свій же файл має читатися");
        assert!(read.paint.is_empty(), "фарба взялася нізвідки");
        assert!(
            plain.to_bytes().len() < model.to_bytes().len(),
            "нефарбований файл не став коротшим"
        );
    }

    /// Фарба не на ту кількість вершин — помилка, а не мовчазна обрізка.
    #[test]
    fn paint_of_the_wrong_length_is_an_error() {
        let message = Model::from_metres(cube(6.0), vec![[1.0, 0.0, 0.0]; 3])
            .expect_err("трьох кольорів на куб мало");
        assert!(message.contains("фарби"), "не те повідомлення: {message}");
    }

    /// Чужий файл, чужа версія й обрізаний файл — це помилки, а не сміття.
    #[test]
    fn a_wrong_file_says_what_is_wrong() {
        let model = Model::from_metres(cube(6.0), Vec::new()).expect("куб — це модель");
        let bytes = model.to_bytes();

        let message = Model::from_bytes(&bytes[..HEADER_BYTES - 1]).expect_err("короткий файл");
        assert!(
            message.contains("коротший"),
            "не те повідомлення: {message}"
        );

        let mut alien = bytes.clone();
        alien[0..8].copy_from_slice(&crate::tiles::MAGIC);
        let message = Model::from_bytes(&alien).expect_err("тайлсет — не меш");
        assert!(message.contains("підпис"), "не те повідомлення: {message}");

        let mut future = bytes.clone();
        future[8..12].copy_from_slice(&(VERSION + 1).to_le_bytes());
        let message = Model::from_bytes(&future).expect_err("чужа версія");
        assert!(message.contains("версія"), "не те повідомлення: {message}");

        let message = Model::from_bytes(&bytes[..bytes.len() - 4]).expect_err("обрізаний файл");
        assert!(message.contains("байтів"), "не те повідомлення: {message}");
    }
}
