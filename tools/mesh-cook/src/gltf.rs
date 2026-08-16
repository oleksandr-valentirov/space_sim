//! Читач glTF — рівно ті ключі, від яких залежить арифметика (T5d2).
//!
//! Правило те саме, що з етикеткою PDS3 у `dem-cook`: чужий формат не
//! розбирається цілком. З glTF беруться геометрія першого примітива й те, що
//! потрібно, щоб її знайти, — акесори, вікна буфера й сам буфер. Матеріали,
//! сцени, вузли, анімації, розширення ігноруються: у гри для них ще немає
//! викликача (CLAUDE.md).
//!
//! ## Що вважається помилкою
//!
//! Усе, чого читач не розуміє, — помилка з поясненням, а не мовчазне
//! спрощення. Файл, у якого два примітиви або чужий тип індексів, читається
//! **неправильно тихо**, і це найгірший вид помилки в ассеті: геометрія
//! приїде правдоподібною.
//!
//! ## Осі
//!
//! Ніяких. Модель робиться носом уздовж `−Y` у Blender, експорт дефолтами
//! кладе ніс у `+Z` glTF — тобто вже в конвенції `Scene::Ship` (виміряно,
//! скіл `blender-assets`). Перестановка осей тут була б другою правдою про
//! ту саму модель.

use engine::sphere::Mesh;
use serde_json::Value;
use std::path::Path;

/// Типи компонентів glTF, які тут щось означають.
const FLOAT: u64 = 5126;
const UNSIGNED_SHORT: u64 = 5123;
const UNSIGNED_INT: u64 = 5125;

/// Габарити, які експортер **сам** записав у JSON акесора.
///
/// Подарунок того самого роду, що `MINIMUM`/`MAXIMUM` в етикетці LOLA: у
/// файлі вже лежить опубліковане число, отримане не нашим парсером. Читач
/// `.bin` мусить його відтворити, і це ловить порядок байтів, тип компонента
/// й забутий `byteOffset`.
#[derive(Debug)]
pub struct Published {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug)]
pub struct Loaded {
    pub mesh: Mesh,
    pub published: Published,
    /// Тип індексів у файлі — щоб кукер міг сказати, що саме прочитав.
    pub index_component: u64,
}

/// Прочитати `.gltf` разом із його `.bin`.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let primitive = root
        .pointer("/meshes/0/primitives")
        .and_then(Value::as_array)
        .ok_or("у файлі немає meshes[0].primitives")?;
    if primitive.len() != 1 {
        return Err(format!(
            "{} примітивів у меші: розбиття за матеріалами — окреме рішення формату, \
             і ухвалювати його треба разом з першим таким матеріалом",
            primitive.len()
        ));
    }
    let primitive = &primitive[0];
    if let Some(mode) = primitive.get("mode").and_then(Value::as_u64) {
        if mode != 4 {
            return Err(format!("mode {mode}, а читач розуміє лише трикутники (4)"));
        }
    }

    let position = accessor_index(primitive, "/attributes/POSITION")?;
    let normal = accessor_index(primitive, "/attributes/NORMAL")?;
    let indices = accessor_index(primitive, "/indices")?;

    let folder = path.parent().unwrap_or(Path::new("."));
    let buffers = read_buffers(&root, folder)?;

    let positions = read_vec3(&root, &buffers, position)?;
    let normals = read_vec3(&root, &buffers, normal)?;
    if positions.len() != normals.len() {
        return Err(format!(
            "{} позицій проти {} нормалей",
            positions.len(),
            normals.len()
        ));
    }
    let (list, index_component) = read_indices(&root, &buffers, indices)?;
    for index in &list {
        if *index as usize >= positions.len() {
            return Err(format!("індекс {index} при {} вершинах", positions.len()));
        }
    }
    if list.len() % 3 != 0 {
        return Err(format!("{} індексів — це не трикутники", list.len()));
    }

    let published = published_bounds(&root, position)?;
    Ok(Loaded {
        mesh: Mesh {
            positions,
            normals: normals
                .iter()
                .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32])
                .collect(),
            indices: list,
        },
        published,
        index_component,
    })
}

fn accessor_index(primitive: &Value, at: &str) -> Result<usize, String> {
    primitive
        .pointer(at)
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .ok_or_else(|| format!("у примітива немає {at}"))
}

/// Буфери файлу. Тільки зовнішні: `data:`-URI тут не буває, бо експорт іде
/// в `GLTF_SEPARATE` — саме заради того, щоб геометрія лежала окремим
/// файлом і читалася без base64.
fn read_buffers(root: &Value, folder: &Path) -> Result<Vec<Vec<u8>>, String> {
    let list = root
        .get("buffers")
        .and_then(Value::as_array)
        .ok_or("у файлі немає buffers")?;
    let mut out = Vec::with_capacity(list.len());
    for (k, buffer) in list.iter().enumerate() {
        let uri = buffer
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("буфер {k} без uri: вбудовані дані не читаються"))?;
        if uri.starts_with("data:") {
            return Err(format!("буфер {k} вбудований у JSON, а очікується .bin"));
        }
        let path = folder.join(uri);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(length) = buffer.get("byteLength").and_then(Value::as_u64) {
            if bytes.len() as u64 != length {
                return Err(format!(
                    "{}: {} байтів проти {length} в JSON",
                    path.display(),
                    bytes.len()
                ));
            }
        }
        out.push(bytes);
    }
    Ok(out)
}

/// Вікно акесора в буфері: зсув, крок і скільки елементів.
struct View<'a> {
    bytes: &'a [u8],
    stride: usize,
    count: usize,
    component: u64,
}

fn view<'a>(
    root: &Value,
    buffers: &'a [Vec<u8>],
    accessor: usize,
    element_bytes: usize,
) -> Result<View<'a>, String> {
    let a = root
        .pointer(&format!("/accessors/{accessor}"))
        .ok_or_else(|| format!("немає акесора {accessor}"))?;
    let count = a
        .get("count")
        .and_then(Value::as_u64)
        .ok_or("акесор без count")? as usize;
    let component = a
        .get("componentType")
        .and_then(Value::as_u64)
        .ok_or("акесор без componentType")?;
    let index = a
        .get("bufferView")
        .and_then(Value::as_u64)
        .ok_or("акесор без bufferView: розріджені акесори не читаються")? as usize;
    let offset = a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;

    let v = root
        .pointer(&format!("/bufferViews/{index}"))
        .ok_or_else(|| format!("немає bufferView {index}"))?;
    let buffer = v.get("buffer").and_then(Value::as_u64).unwrap_or(0) as usize;
    let view_offset = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let stride = v
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(element_bytes);
    let bytes = buffers
        .get(buffer)
        .ok_or_else(|| format!("bufferView {index} дивиться в буфер {buffer}, якого немає"))?;

    let start = view_offset + offset;
    let need = start + (count - 1) * stride + element_bytes;
    if count > 0 && bytes.len() < need {
        return Err(format!(
            "акесор {accessor} вимагає {need} байтів, а в буфері {}",
            bytes.len()
        ));
    }
    Ok(View {
        bytes: &bytes[start..],
        stride,
        count,
        component,
    })
}

fn read_vec3(root: &Value, buffers: &[Vec<u8>], accessor: usize) -> Result<Vec<[f64; 3]>, String> {
    let v = view(root, buffers, accessor, 12)?;
    if v.component != FLOAT {
        return Err(format!(
            "акесор {accessor}: componentType {}, а VEC3 очікується у float",
            v.component
        ));
    }
    let float = |at: usize| {
        f32::from_le_bytes([
            v.bytes[at],
            v.bytes[at + 1],
            v.bytes[at + 2],
            v.bytes[at + 3],
        ])
    };
    let mut out = Vec::with_capacity(v.count);
    for k in 0..v.count {
        let at = k * v.stride;
        out.push([
            f64::from(float(at)),
            f64::from(float(at + 4)),
            f64::from(float(at + 8)),
        ]);
    }
    Ok(out)
}

/// Індекси **обох** типів.
///
/// `UNSIGNED_SHORT` з'являється сам, доки вершин менше 65 536, а `UNSIGNED_INT`
/// — щойно їх більше. Читач, який знає лише один із них, працює рівно доти,
/// доки модель не підросла, і ламається на найгіршому кроці: коли міняли
/// форму, а не код.
fn read_indices(
    root: &Value,
    buffers: &[Vec<u8>],
    accessor: usize,
) -> Result<(Vec<u32>, u64), String> {
    let head = root
        .pointer(&format!("/accessors/{accessor}/componentType"))
        .and_then(Value::as_u64)
        .ok_or("акесор індексів без componentType")?;
    let width = match head {
        UNSIGNED_SHORT => 2,
        UNSIGNED_INT => 4,
        other => {
            return Err(format!(
                "індекси з componentType {other}: читач розуміє 5123 і 5125"
            ))
        }
    };
    let v = view(root, buffers, accessor, width)?;
    let mut out = Vec::with_capacity(v.count);
    for k in 0..v.count {
        let at = k * v.stride;
        out.push(match width {
            2 => u32::from(u16::from_le_bytes([v.bytes[at], v.bytes[at + 1]])),
            _ => u32::from_le_bytes([
                v.bytes[at],
                v.bytes[at + 1],
                v.bytes[at + 2],
                v.bytes[at + 3],
            ]),
        });
    }
    Ok((out, head))
}

fn published_bounds(root: &Value, accessor: usize) -> Result<Published, String> {
    let read = |key: &str| -> Result<[f64; 3], String> {
        let list = root
            .pointer(&format!("/accessors/{accessor}/{key}"))
            .and_then(Value::as_array)
            .ok_or_else(|| format!("акесор позицій без {key}"))?;
        if list.len() != 3 {
            return Err(format!("{key} з {} чисел, а не з трьох", list.len()));
        }
        let mut out = [0.0; 3];
        for (k, value) in list.iter().enumerate() {
            out[k] = value
                .as_f64()
                .ok_or_else(|| format!("{key}[{k}] не число"))?;
        }
        Ok(out)
    };
    Ok(Published {
        min: read("min")?,
        max: read("max")?,
    })
}
