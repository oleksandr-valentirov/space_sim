//! A glTF reader -- exactly the keys the arithmetic depends on (T5d2).
//!
//! Same rule as the PDS3 label in `dem-cook`: someone else's format is not
//! parsed in full. From glTF we take the first primitive's geometry and what
//! is needed to find it -- accessors, buffer views and the buffer itself.
//! Materials, scenes, nodes, animations and extensions are ignored: the game
//! has no caller for them yet (CLAUDE.md).
//!
//! ## What counts as an error
//!
//! Anything the reader does not understand is an error with an explanation,
//! not a silent simplification. A file with two primitives or an unfamiliar
//! index type reads **wrongly and quietly**, the worst kind of asset bug: the
//! geometry arrives looking plausible.
//!
//! ## Axes
//!
//! None. The model is built nose along `-Y` in Blender, and the export
//! defaults put the nose at glTF `+Z` -- already the `Scene::Ship` convention
//! (measured, skill `blender-assets`). Permuting axes here would be a second
//! truth about the same model.

use engine::sphere::Mesh;
use serde_json::Value;
use std::path::Path;

/// The glTF component types that mean something here.
const FLOAT: u64 = 5126;
const UNSIGNED_BYTE: u64 = 5121;
const UNSIGNED_SHORT: u64 = 5123;
const UNSIGNED_INT: u64 = 5125;

/// Bounds the exporter wrote into the accessor JSON **itself**.
///
/// A gift of the same kind as `MINIMUM`/`MAXIMUM` in the LOLA label: the file
/// already holds a published number produced by someone else's parser. The
/// `.bin` reader must reproduce it, and that catches byte order, component
/// type and a forgotten `byteOffset`.
#[derive(Debug)]
pub struct Published {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug)]
pub struct Loaded {
    pub mesh: Mesh,
    pub published: Published,
    /// Index type in the file, so the cooker can say what it read.
    pub index_component: u64,
    /// `COLOR_0` if the file has one; empty means the model is unpainted.
    pub paint: Vec<[f32; 3]>,
}

/// Read a `.gltf` together with its `.bin`.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let primitive = root
        .pointer("/meshes/0/primitives")
        .and_then(Value::as_array)
        .ok_or("the file has no meshes[0].primitives")?;
    if primitive.len() != 1 {
        return Err(format!(
            "{} primitives in the mesh: splitting by material is its own format \
             decision, to be taken alongside the first such material",
            primitive.len()
        ));
    }
    let primitive = &primitive[0];
    if let Some(mode) = primitive.get("mode").and_then(Value::as_u64) {
        if mode != 4 {
            return Err(format!(
                "mode {mode}, but the reader understands only triangles (4)"
            ));
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
            "{} positions against {} normals",
            positions.len(),
            normals.len()
        ));
    }
    let (list, index_component) = read_indices(&root, &buffers, indices)?;
    for index in &list {
        if *index as usize >= positions.len() {
            return Err(format!("index {index} with {} vertices", positions.len()));
        }
    }
    if list.len() % 3 != 0 {
        return Err(format!("{} indices are not triangles", list.len()));
    }

    // Paint is optional: a model without it is a pre-T9b model, and the reader
    // must read it exactly as it did then.
    let paint = match accessor_index(primitive, "/attributes/COLOR_0") {
        Ok(colour) => {
            let read = read_colour(&root, &buffers, colour)?;
            if read.len() != positions.len() {
                return Err(format!(
                    "{} colours against {} positions",
                    read.len(),
                    positions.len()
                ));
            }
            read
        }
        Err(_) => Vec::new(),
    };

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
        paint,
    })
}

fn accessor_index(primitive: &Value, at: &str) -> Result<usize, String> {
    primitive
        .pointer(at)
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .ok_or_else(|| format!("the primitive has no {at}"))
}

/// The file's buffers. External only: no `data:` URI appears here, because the
/// export uses `GLTF_SEPARATE` precisely so the geometry sits in its own file
/// and is read without base64.
fn read_buffers(root: &Value, folder: &Path) -> Result<Vec<Vec<u8>>, String> {
    let list = root
        .get("buffers")
        .and_then(Value::as_array)
        .ok_or("the file has no buffers")?;
    let mut out = Vec::with_capacity(list.len());
    for (k, buffer) in list.iter().enumerate() {
        let uri = buffer
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("buffer {k} has no uri: embedded data is not read"))?;
        if uri.starts_with("data:") {
            return Err(format!(
                "buffer {k} is embedded in JSON, but a .bin is expected"
            ));
        }
        let path = folder.join(uri);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(length) = buffer.get("byteLength").and_then(Value::as_u64) {
            if bytes.len() as u64 != length {
                return Err(format!(
                    "{}: {} bytes against {length} in JSON",
                    path.display(),
                    bytes.len()
                ));
            }
        }
        out.push(bytes);
    }
    Ok(out)
}

/// An accessor's window into the buffer: offset, stride and element count.
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
        .ok_or_else(|| format!("no accessor {accessor}"))?;
    let count = a
        .get("count")
        .and_then(Value::as_u64)
        .ok_or("accessor without count")? as usize;
    let component = a
        .get("componentType")
        .and_then(Value::as_u64)
        .ok_or("accessor without componentType")?;
    let index =
        a.get("bufferView")
            .and_then(Value::as_u64)
            .ok_or("accessor without bufferView: sparse accessors are not read")? as usize;
    let offset = a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;

    let v = root
        .pointer(&format!("/bufferViews/{index}"))
        .ok_or_else(|| format!("no bufferView {index}"))?;
    let buffer = v.get("buffer").and_then(Value::as_u64).unwrap_or(0) as usize;
    let view_offset = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let stride = v
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(element_bytes);
    let bytes = buffers.get(buffer).ok_or_else(|| {
        format!("bufferView {index} points at buffer {buffer}, which does not exist")
    })?;

    let start = view_offset + offset;
    let need = start + (count - 1) * stride + element_bytes;
    if count > 0 && bytes.len() < need {
        return Err(format!(
            "accessor {accessor} needs {need} bytes, but the buffer has {}",
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

/// `COLOR_0` is base colour per vertex, **linear light** per the glTF spec --
/// exactly what the frame paints with, so it needs no conversion.
///
/// Three representations instead of one is not the reader's whim: Blender
/// chooses what to write the colour as, and for this model chose normalised
/// `UNSIGNED_SHORT`. Accepting only `float` would mean a reader that breaks
/// because someone flipped an attribute type in the `.blend`.
///
/// Alpha is discarded: it is one everywhere in the model, and hull
/// transparency would be its own render pass, not a channel in the colour.
fn read_colour(
    root: &Value,
    buffers: &[Vec<u8>],
    accessor: usize,
) -> Result<Vec<[f32; 3]>, String> {
    let a = root
        .pointer(&format!("/accessors/{accessor}"))
        .ok_or_else(|| format!("no accessor {accessor}"))?;
    let channels = match a.get("type").and_then(Value::as_str) {
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        other => return Err(format!("COLOR_0 of type {other:?}, but it is VEC3 or VEC4")),
    };
    let normalized = a
        .get("normalized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let component = a
        .get("componentType")
        .and_then(Value::as_u64)
        .ok_or("accessor without componentType")?;
    let (size, scale) = match component {
        FLOAT => (4, 1.0),
        UNSIGNED_SHORT if normalized => (2, 1.0 / 65535.0),
        UNSIGNED_BYTE if normalized => (1, 1.0 / 255.0),
        other => {
            return Err(format!(
                "COLOR_0: componentType {other}, normalized {normalized} — \
                 the reader understands float and normalised ushort/ubyte"
            ))
        }
    };

    let v = view(root, buffers, accessor, channels * size)?;
    let mut out = Vec::with_capacity(v.count);
    for k in 0..v.count {
        let at = k * v.stride;
        let mut colour = [0.0f32; 3];
        for (c, value) in colour.iter_mut().enumerate() {
            let byte = at + c * size;
            *value = match component {
                FLOAT => f32::from_le_bytes([
                    v.bytes[byte],
                    v.bytes[byte + 1],
                    v.bytes[byte + 2],
                    v.bytes[byte + 3],
                ]),
                UNSIGNED_SHORT => {
                    f32::from(u16::from_le_bytes([v.bytes[byte], v.bytes[byte + 1]])) * scale
                }
                _ => f32::from(v.bytes[byte]) * scale,
            };
        }
        out.push(colour);
    }
    Ok(out)
}

fn read_vec3(root: &Value, buffers: &[Vec<u8>], accessor: usize) -> Result<Vec<[f64; 3]>, String> {
    let v = view(root, buffers, accessor, 12)?;
    if v.component != FLOAT {
        return Err(format!(
            "accessor {accessor}: componentType {}, but VEC3 is expected as float",
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

/// Indices of **both** types.
///
/// `UNSIGNED_SHORT` appears on its own while there are fewer than 65,536
/// vertices, and `UNSIGNED_INT` as soon as there are more. A reader knowing
/// only one works exactly until the model grows, and breaks at the worst step:
/// when the shape changed, not the code.
fn read_indices(
    root: &Value,
    buffers: &[Vec<u8>],
    accessor: usize,
) -> Result<(Vec<u32>, u64), String> {
    let head = root
        .pointer(&format!("/accessors/{accessor}/componentType"))
        .and_then(Value::as_u64)
        .ok_or("index accessor without componentType")?;
    let width = match head {
        UNSIGNED_SHORT => 2,
        UNSIGNED_INT => 4,
        other => {
            return Err(format!(
                "indices with componentType {other}: the reader understands 5123 and 5125"
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
            .ok_or_else(|| format!("position accessor without {key}"))?;
        if list.len() != 3 {
            return Err(format!("{key} has {} numbers, not three", list.len()));
        }
        let mut out = [0.0; 3];
        for (k, value) in list.iter().enumerate() {
            out[k] = value
                .as_f64()
                .ok_or_else(|| format!("{key}[{k}] is not a number"))?;
        }
        Ok(out)
    };
    Ok(Published {
        min: read("min")?,
        max: read("max")?,
    })
}
