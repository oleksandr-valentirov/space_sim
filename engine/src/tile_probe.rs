//! Скільки тайлів витримує bindless-масив, і що вони коштують (етап T, T2).
//!
//! Крок T2 має назвати рівень колірної піраміди **числом**, і три числа для
//! цього названі наперед: тайлів `6·(4⁰+…+4^(L−1))`, байтів на тайл, і
//! скільки текстур витримує масив. Перші два — арифметика; третє — властивість
//! пристрою, і саме його припустити не можна.
//!
//! cargo run --release -p engine -- --tile-probe
//!
//! ## Чому заявленого ліміту недостатньо
//!
//! `max_binding_array_elements_per_shader_stage` виміряний `gpu-probe` на цій
//! машині — **1 048 576** (NVIDIA), **8 388 606** (RADV), **1 000 000**
//! (llvmpipe). Найменше з трьох — на **тридцять разів** більше за найглибшу
//! піраміду, яку ми взагалі розглядаємо. Якби питання було в ньому, воно було
//! б закрите ще на етапі E.
//!
//! Упирається воно в інше, і в те, чого адаптер про себе не каже:
//!
//! - **гранулярність алокації.** Тайл — це 35×35 текселів, тобто одиниці
//!   кілобайтів. Драйвер роздає пам'ять блоками, і скільки з блока
//!   пропадає, видно лише з `generate_allocator_report()`: там є і сума
//!   алокацій, і сума **зарезервованого**. Різниця між ними — це і є ціна
//!   дрібності, яку не можна порахувати з формату файлу;
//! - **час.** Тридцять тисяч викликів `create_texture` — це тридцять тисяч
//!   об'єктів драйвера, і платиться цей час при завантаженні тіла, а не раз
//!   на життя процесу.
//!
//! ## Чому зонд робить свій пристрій, і чому на кожному адаптері
//!
//! `Gpu::new` просить рівно `max(default, 4096)` елементів — число, узяте під
//! 2046 тайлів рельєфу (`gpu.rs`). Зонд, що міряв би межу через нього, міряв
//! би цю константу, а не пристрій. Тому тут — власний пристрій із лімітом, що
//! дорівнює адаптерному.
//!
//! Адаптери беруться **всі апаратні**, а не один найшвидший, і саме тому, що
//! питання про пам'ять: у дискретної карти своя VRAM, у інтегрованої — та
//! сама системна, з якої вже живе гра. Один рядок з дискретної відповів би на
//! половину питання. Програмні адаптери пропускаються: у llvmpipe «пам'ять
//! GPU» — це malloc, тобто число, яке нічого не обмежує.
//!
//! ## Що виміряно (2026-08-16)
//!
//! **Тайл коштує ×3.34 від того, що несе, і це число те саме на обох
//! вендорах, у всіх трьох форматів і на всіх трьох глибинах піраміди.** Не
//! залежить воно ні від кількості тайлів, ні від того, скільки в тайлі
//! каналів: 1225 байтів даних у `R8Unorm` перетворюються на 4096 у пам'яті,
//! 2450 у `R16Sint` — на 8192. Тобто **35×35 — це не «маленька текстура», це
//! чотирикілобайтова**, і саме ця константа, а не заявлена стеля масиву,
//! вирішує, скільки рівнів піраміди можна собі дозволити.
//!
//! Єдина розбіжність між вендорами — `Rgba8Unorm`: NVIDIA бере 12 288 байтів
//! на тайл, RADV — 16 384. Це ще один доказ, що трибайтовий колір нічого не
//! економить: те, що в файлі важить 3 байти на вузол, у пам'яті коштує 12–16
//! кілобайтів на тайл проти 4 в одноканального.
//!
//! Резерв росте сходинками й на дрібних пірамідах дорівнює нулю (нові
//! текстури влазять у вже взятий блок), а на рівні 7 подвоює рахунок:
//! 128 МіБ алокацій → **256 МіБ зарезервованих**. Час створення лінійний за
//! кількістю тайлів: ~11 мс на 2046, ~38 мс на 8190, ~165 мс на 32 766.
//!
//! ## Що виміряно для W1: ціна запеченого нахилу (2026-08-16)
//!
//! **Ореол не коштує нічого.** 33×33 і 35×35 дають той самий байт на тайл — у
//! кожному форматі, на кожній глибині, на обох вендорах. Тобто 12.5%, які
//! ореол важить у файлі, у пам'яті GPU не існують узагалі: обидві сітки
//! потрапляють в один блок гранулярності. Викидати ореол з формату можна
//! заради простоти й диска, але **не заради пам'яті** — там купувати нічого.
//!
//! **Другий канал коштує рівно один крок гранулярності, і вендори різні:**
//!
//! | формат | NVIDIA | RADV |
//! |---|---|---|
//! | `R16Sint` (висоти нині) | 8192 | 8192 |
//! | `Rg16Sint` (висоти + нахил) | **12 288** | **16 384** |
//!
//! Тобто ×1.5 на дискретній карті й ×2 на інтегрованій, і саме інтегрована
//! платить із тієї ж пам'яті, з якої живе гра. У числах сцени: Місяць
//! 16 → 24/32 МіБ, Земля 64 → 96/128 МіБ, разом **+40 МіБ (NVIDIA)** і
//! **+80 МіБ (RADV)**. Це і є ціна відповіді на Q3, названа до того, як
//! формат змінився.
//!
//! ⚠ **Час прив'язки масиву від формату не залежить взагалі** — 1.0–1.1 мс на
//! 8190 текстур у всіх сімох рядків. Це саме те, що каже борг D19: платить
//! драйвер за **кількість** текстур, а не за їхній розмір, тож запечений нахил
//! D19 не погіршує ні на мікросекунду.

use std::time::Instant;

use crate::tiles;

/// Рівні, які має сенс міряти: 5 — теперішній рельєф, 6 і 7 — кандидати на
/// колір (3.8 км і 1.9 км на вузол Місяця).
const LEVELS: [u32; 3] = [5, 6, 7];

/// Формати й сітки, між якими вибирають T2 і W1.
///
/// `R16Sint` — те, що несуть висоти сьогодні, і воно тут заради масштабу
/// порівняння. `R8Unorm` — один канал: глобальна мозаїка LROC WAC монохромна,
/// тобто для Місяця це не спрощення, а рівно те, що є в джерелі. `Rgba8Unorm` —
/// чотири, бо трибайтового формату текстури в wgpu **немає взагалі**: `Rgb8` не
/// існує ні в WebGPU, ні в Vulkan як формат, який можна семплювати без
/// розширень. Тобто «35²·3» з роадмапу — це розмір у файлі, а не в пам'яті GPU.
///
/// `Rg16Sint` додано для W1: запечений нахил (Q3, варіант 1) кладе в той самий
/// тексель другий `i16`, і питання рівно одне — чи подвоює це пам'ять, чи
/// гранулярність з'їдає різницю так само, як вона з'їдає ореол.
///
/// **Сітка — вимір таблиці, а не константа**, і теж через W1: після запікання
/// нахилу ореол не читає ніхто, тож 33×33 стає можливим. Чи варте воно версії
/// обох форматів — вирішує рядок «байт на тайл», а не арифметика над файлом.
const FORMATS: [(&str, wgpu::TextureFormat, usize, usize); 7] = [
    (
        "R16Sint 35² (висоти нині)",
        wgpu::TextureFormat::R16Sint,
        2,
        tiles::STORED,
    ),
    (
        "R16Sint 33² (без ореолу)",
        wgpu::TextureFormat::R16Sint,
        2,
        tiles::NODES,
    ),
    (
        "Rg16Sint 35² (з нахилом)",
        wgpu::TextureFormat::Rg16Sint,
        4,
        tiles::STORED,
    ),
    (
        "Rg16Sint 33² (нахил, без ореолу)",
        wgpu::TextureFormat::Rg16Sint,
        4,
        tiles::NODES,
    ),
    (
        "R8Unorm 35² (колір Місяця)",
        wgpu::TextureFormat::R8Unorm,
        1,
        tiles::STORED,
    ),
    (
        "Rgba8Unorm 35² (колір Землі)",
        wgpu::TextureFormat::Rgba8Unorm,
        4,
        tiles::STORED,
    ),
    (
        "Rgba8Unorm 33² (без ореолу)",
        wgpu::TextureFormat::Rgba8Unorm,
        4,
        tiles::NODES,
    ),
];

struct Row {
    format: &'static str,
    levels: u32,
    tiles: usize,
    data_mib: f64,
    allocated_mib: f64,
    reserved_mib: f64,
    create_ms: f64,
    bind_ms: f64,
    failed: Option<String>,
}

/// Порахувати й надрукувати таблицю по кожному апаратному адаптеру.
pub fn report() -> Result<(), String> {
    let instance = wgpu::Instance::default();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));

    let mut measured = 0;
    for adapter in &adapters {
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            continue;
        }
        match one_adapter(adapter) {
            Ok(()) => measured += 1,
            Err(e) => println!("{}: пропущено — {e}\n", adapter.get_info().name),
        }
    }

    if measured == 0 {
        return Err("жоден апаратний адаптер не зміряний".to_string());
    }
    Ok(())
}

/// Таблиця для одного адаптера.
fn one_adapter(adapter: &wgpu::Adapter) -> Result<(), String> {
    let info = adapter.get_info();
    let ceiling = adapter.limits().max_binding_array_elements_per_shader_stage;
    println!(
        "адаптер: {:?} — {} ({:?})",
        info.backend, info.name, info.device_type
    );
    println!("заявлена стеля масиву: {ceiling} елементів");
    if ceiling == 0 {
        return Err("адаптер не має масивів прив'язок — міряти нема чого".to_string());
    }

    let wanted = wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
        | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY;
    if !adapter.features().contains(wanted) {
        return Err("немає повного набору bindless".to_string());
    }

    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_binding_array_elements_per_shader_stage = ceiling;
    limits.max_binding_array_sampler_elements_per_shader_stage = adapter
        .limits()
        .max_binding_array_sampler_elements_per_shader_stage;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tile probe"),
        required_features: wanted,
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .map_err(|e| format!("пристрій не створюється: {e}"))?;

    let mut rows = Vec::new();
    for (name, format, bytes_per_texel, side) in FORMATS {
        for levels in LEVELS {
            rows.push(measure(
                &device,
                &queue,
                name,
                format,
                bytes_per_texel,
                side,
                levels,
            ));
        }
    }

    print_table(&rows);
    print_verdict(&rows);
    println!();
    Ok(())
}

/// Один рядок таблиці: створити піраміду тайлів, зібрати з них масив, зміряти.
///
/// Текстури живуть до кінця функції й помирають разом із нею — наступний
/// рядок мусить починати з чистого аркуша, інакше звіт алокатора показував би
/// суму всіх попередніх вимірів.
fn measure(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format_name: &'static str,
    format: wgpu::TextureFormat,
    bytes_per_texel: usize,
    nodes: usize,
    levels: u32,
) -> Row {
    let tiles_count = tiles::Terrain::count(levels);
    let side = nodes as u32;
    let data_bytes = tiles_count * nodes * nodes * bytes_per_texel;
    let pixels = vec![0u8; nodes * nodes * bytes_per_texel];

    let before = memory(device);
    let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);

    let start = Instant::now();
    let mut textures = Vec::with_capacity(tiles_count);
    let mut views = Vec::with_capacity(tiles_count);
    for _ in 0..tiles_count {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * bytes_per_texel as u32),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
        views.push(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        textures.push(texture);
    }
    // Черга віддає вивантаження ліниво, тож без цього час і пам'ять зонда
    // виявилися б часом і пам'яттю самого запису в чергу.
    queue.submit(std::iter::empty());
    let _ = device.poll(wait());
    let create_ms = start.elapsed().as_secs_f64() * 1e3;

    let start = Instant::now();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: sample_type(format),
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: std::num::NonZeroU32::new(tiles_count as u32),
        }],
    });
    let borrowed: Vec<&wgpu::TextureView> = views.iter().collect();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureViewArray(&borrowed),
        }],
    });
    let _ = device.poll(wait());
    let bind_ms = start.elapsed().as_secs_f64() * 1e3;

    let after = memory(device);
    let allocated = after.0.saturating_sub(before.0);
    let reserved = after.1.saturating_sub(before.1);
    let failed = pollster::block_on(scope.pop()).map(|e| e.to_string());

    drop(group);
    drop(views);
    drop(textures);
    let _ = device.poll(wait());

    Row {
        format: format_name,
        levels,
        tiles: tiles_count,
        data_mib: data_bytes as f64 / (1024.0 * 1024.0),
        allocated_mib: allocated as f64 / (1024.0 * 1024.0),
        reserved_mib: reserved as f64 / (1024.0 * 1024.0),
        create_ms,
        bind_ms,
        failed,
    }
}

/// `poll(Wait)` без обмежень — той самий виклик, що в `shot.rs`.
fn wait() -> wgpu::PollType {
    wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    }
}

fn sample_type(format: wgpu::TextureFormat) -> wgpu::TextureSampleType {
    match format {
        wgpu::TextureFormat::R16Sint | wgpu::TextureFormat::Rg16Sint => {
            wgpu::TextureSampleType::Sint
        }
        _ => wgpu::TextureSampleType::Float { filterable: true },
    }
}

/// Два числа пам'яті: сума алокацій і сума зарезервованого, байти.
///
/// Обидва, і це не надмір — вони відповідають на різні питання, і **перше з
/// них зонд спершу не рахував і через це побачив нулі**. Резерв росте
/// **блоками**: доки нові текстури влазять у вже взятий блок, різниця «до» й
/// «після» дорівнює нулю, і рядок виглядає безкоштовним, хоча пам'ять
/// витрачена. Сума алокацій такої дірки не має — вона рахує кожну текстуру
/// окремо, разом із вирівнюванням, тобто саме ту гранулярність, заради якої
/// зонд і писався.
///
/// Резерв лишається другим стовпцем, бо він каже інше: скільки пристрій
/// **тримає** під нас, включно з нічим не зайнятими хвостами блоків. Бекенд
/// без звіту (GL) дає нулі, і в таблиці це видно як нулі, а не як
/// «нічого не коштує».
fn memory(device: &wgpu::Device) -> (u64, u64) {
    device
        .generate_allocator_report()
        .map(|report| (report.total_allocated_bytes, report.total_reserved_bytes))
        .unwrap_or((0, 0))
}

fn print_table(rows: &[Row]) {
    println!();
    println!(
        "формат                           рівнів  тайлів   дані, МіБ  алок., МіБ  резерв, МіБ  ств., мс  масив, мс"
    );
    for row in rows {
        println!(
            "{:32} {:6}  {:6}   {:9.2}  {:10.2}  {:11.2}  {:8.1}  {:9.1}{}",
            row.format,
            row.levels,
            row.tiles,
            row.data_mib,
            row.allocated_mib,
            row.reserved_mib,
            row.create_ms,
            row.bind_ms,
            match &row.failed {
                Some(e) => format!("  ← {e}"),
                None => String::new(),
            }
        );
    }
}

/// Висновок, а не лише дані — те саме правило, що в `gpu-probe`.
fn print_verdict(rows: &[Row]) {
    println!();
    println!("Висновок\n");

    for row in rows {
        if row.failed.is_some() {
            println!(
                "  {} на {} рівнях ({} тайлів) НЕ ВЛІЗ",
                row.format, row.levels, row.tiles
            );
        }
    }

    // Накладка гранулярності — головне число зонда: воно каже, скільки пам'яті
    // з'їдає сама дрібність тайла, і саме воно, а не заявлена стеля, обмежує
    // глибину піраміди.
    for row in rows {
        if row.data_mib > 0.0 && row.allocated_mib > 0.0 {
            println!(
                "  {} / {} рівнів: алокація ×{:.2} від даних, {:.0} байт на тайл",
                row.format,
                row.levels,
                row.allocated_mib / row.data_mib,
                row.allocated_mib * 1024.0 * 1024.0 / row.tiles as f64
            );
        }
    }
}
