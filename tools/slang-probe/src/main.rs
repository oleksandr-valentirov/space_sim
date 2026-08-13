//! Розвідка P1: чи доїжджає вихід Slang до wgpu (ROADMAP, етап E).
//!
//! Питання не «чи компілюється». Компілюється майже завжди; ламається на
//! стику. Тому зонд проходить увесь ланцюжок і зупиняється лише на останньому
//! кроці:
//!
//!   1. `slangc` перетворює один `.slang` на WGSL і на SPIR-V;
//!   2. wgpu приймає кожен як модуль шейдера;
//!   3. з нього збирається пайплайн;
//!   4. він **малює** трикутник у текстуру 64×64;
//!   5. пікселі читаються назад і звіряються з тим, що мало вийти.
//!
//! Крок 5 і є суттю. Модуль, який створився й не намалював нічого, — це
//! пройдена перевірка й зламаний рендер; саме так виглядає неспівпадіння
//! семантик або порядку локацій. Тому перевіряється колір у трьох точках, а
//! не факт відсутності помилки.
//!
//! Два шляхи навмисно, бо ROADMAP P1 каже, що через SPIR-V «працює лише в
//! частині конфігурацій», і треба знати, у яких саме:
//!
//!   WGSL     `slangc -target wgsl`  — naga розбирає як звичайний текст;
//!   SPIR-V   `slangc -target spirv` — naga розбирає бінарник (фіча `spirv`).
//!
//! Запускається з кореня репозиторію:
//!
//!     sh scripts/fetch_slang.sh     один раз
//!     cargo run -p slang-probe

use std::path::{Path, PathBuf};
use std::process::Command;

const SHADERS: &str = "tools/slang-probe/shaders";

/// Що саме перевіряється. `draws` розрізняє два різні питання: «доїжджає до
/// картинки» і «приймається взагалі». Третій випадок малювати не може —
/// йому потрібні вершинні буфери, — але він і не про це.
struct Case {
    label: &'static str,
    shader: &'static str,
    target: &'static str,
    extension: &'static str,
    draws: bool,
}

const CASES: &[Case] = &[
    Case {
        label: "WGSL",
        shader: "triangle.slang",
        target: "wgsl",
        extension: "wgsl",
        draws: true,
    },
    Case {
        label: "SPIR-V",
        shader: "triangle.slang",
        target: "spirv",
        extension: "spv",
        draws: true,
    },
    Case {
        label: "SPIR-V без SV_VertexID",
        shader: "vertex_buffer.slang",
        target: "spirv",
        extension: "novid.spv",
        draws: false,
    },
];
const SLANGC: &str = "tools/slang/bin/slangc";
const OUT_DIR: &str = "build/slang";

const SIZE: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Точки, у яких перевіряється результат, і який канал там має переважати.
///
/// Перша версія цих точок стояла рівно на вершинах трикутника — і перевірка
/// падала на цілком правильному рендері: пікселя точно на вершині
/// растеризатор законно не зафарбовує. Тому точки зсунуті всередину.
///
/// Кутики не менш важливі за нутро: якби трикутник вийшов на весь екран або
/// перевернувся, перевірка «щось намалювалось» усе одно пройшла б.
const INSIDE: &[(&str, u32, u32, usize)] = &[
    ("верхівка — червона", SIZE / 2, SIZE / 3, 0),
    ("лівий низ — зелений", SIZE / 3, (SIZE * 5) / 7, 1),
    ("правий низ — синій", (SIZE * 2) / 3, (SIZE * 5) / 7, 2),
];

const BACKGROUND: &[(&str, u32, u32)] = &[
    ("лівий верхній кут", 1, 1),
    ("правий верхній кут", SIZE - 2, 1),
    ("лівий нижній кут", 1, SIZE - 2),
];

/// Наскільки очікуваний канал має переважати решту.
///
/// Перевіряється переважання, а не близькість до чистого кольору: трикутник
/// інтерполює вершинні кольори, тож усередині вони змішані, і будь-який поріг
/// «схоже на червоний» був би підгонкою під конкретні координати.
const DOMINANCE: u8 = 60;

struct Outcome {
    label: &'static str,
    draws: bool,
    compiled: Result<PathBuf, String>,
    accepted: Result<(), String>,
}

fn main() {
    let version = std::fs::read_to_string(Path::new(SLANGC).parent().unwrap().join("../VERSION"))
        .unwrap_or_else(|_| "невідома".into());

    if !Path::new(SLANGC).exists() {
        eprintln!("немає {SLANGC}");
        eprintln!("  спершу: sh scripts/fetch_slang.sh");
        eprintln!("  запускати з кореня репозиторію");
        std::process::exit(1);
    }

    println!("Slang {}", version.trim());
    println!("шейдери: {SHADERS}/\n");

    let (device, queue, adapter) = match open_device() {
        Some(triple) => triple,
        None => {
            eprintln!("немає адаптера, з яким можна створити пристрій");
            std::process::exit(1);
        }
    };

    let info = adapter.get_info();
    println!(
        "адаптер: {:?} — {} ({:?})\n",
        info.backend, info.name, info.device_type
    );

    let mut outcomes = Vec::new();

    for case in CASES {
        let compiled = compile(case);
        let accepted = match &compiled {
            Ok(file) => match load(&device, case.target, file) {
                Ok(module) if case.draws => draw_and_check(&device, &queue, &module),
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Err(e) => Err(format!("не дійшло до завантаження: {e}")),
        };

        outcomes.push(Outcome {
            label: case.label,
            draws: case.draws,
            compiled,
            accepted,
        });
    }

    report(&outcomes);

    // Провалом вважається лише те, що ЖОДЕН шлях не працює: саме на цей
    // випадок ROADMAP P1 має розвилку «писати WGSL руками до M4». Один
    // робочий шлях — це успіх розвідки, а не половина.
    if outcomes
        .iter()
        .filter(|o| o.draws)
        .all(|o| o.accepted.is_err())
    {
        std::process::exit(1);
    }
}

fn open_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::Adapter)> {
    let instance = wgpu::Instance::default();

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("slang-probe"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .ok()?;

    Some((device, queue, adapter))
}

fn compile(case: &Case) -> Result<PathBuf, String> {
    std::fs::create_dir_all(OUT_DIR).map_err(|e| e.to_string())?;
    let stem = case.shader.trim_end_matches(".slang");
    let out = PathBuf::from(OUT_DIR).join(format!("{stem}.{}", case.extension));

    let result = Command::new(SLANGC)
        .arg(Path::new(SHADERS).join(case.shader))
        .args(["-target", case.target])
        .arg("-o")
        .arg(&out)
        .output()
        .map_err(|e| format!("не запускається slangc: {e}"))?;

    if !result.status.success() {
        return Err(format!(
            "slangc -target {} повернув {}:\n{}",
            case.target,
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }

    Ok(out)
}

fn load(device: &wgpu::Device, target: &str, file: &Path) -> Result<wgpu::ShaderModule, String> {
    // Помилка розбору шейдера в wgpu приходить через обробник помилок
    // пристрою, а не як Result. Без цього хомута зламаний модуль впав би
    // панікою десь пізніше, і зонд повідомив би не те місце.
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let module = if target == "wgsl" {
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slang-wgsl"),
            source: wgpu::ShaderSource::Wgsl(text.into()),
        })
    } else {
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slang-spirv"),
            source: wgpu::ShaderSource::SpirV(words.into()),
        })
    };

    match pollster::block_on(scope.pop()) {
        Some(error) => Err(format!("wgpu не прийняв модуль: {error}")),
        None => Ok(module),
    }
}

fn draw_and_check(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    module: &wgpu::ShaderModule,
) -> Result<(), String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        // Push-константи в wgpu 30 називаються immediates; нам їх не треба.
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("triangle"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(format!("пайплайн не зібрався: {error}"));
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // 64 пікселі по 4 байти — рівно 256, тобто вимога вирівнювання рядка
    // виконується без доповнення. Саме тому розмір такий, а не 100×100.
    let bytes = (SIZE * SIZE * 4) as u64;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("triangle"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Чорний фон: перевірки кутиків спираються на нього.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );

    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| format!("не дочекалися GPU: {e}"))?;

    let data = slice
        .get_mapped_range()
        .map_err(|e| format!("буфер не відобразився: {e}"))?;
    let pixels = data.to_vec();
    drop(data);
    readback.unmap();

    verify(&pixels)
}

fn verify(pixels: &[u8]) -> Result<(), String> {
    let mut problems = Vec::new();

    let pixel = |x: u32, y: u32| -> [u8; 3] {
        let offset = ((y * SIZE + x) * 4) as usize;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    };

    for (label, x, y, channel) in INSIDE {
        let got = pixel(*x, *y);
        let mine = got[*channel];

        let beaten = got
            .iter()
            .enumerate()
            .filter(|(i, _)| i != channel)
            .all(|(_, &other)| mine > other && mine - other >= DOMINANCE);

        if !beaten {
            problems.push(format!(
                "{label}: канал {channel} не переважає, маємо {got:?}"
            ));
        }
    }

    for (label, x, y) in BACKGROUND {
        let got = pixel(*x, *y);
        if got != [0, 0, 0] {
            problems.push(format!("{label}: мав лишитися фоном, маємо {got:?}"));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

fn report(outcomes: &[Outcome]) {
    println!("Результат\n");

    for outcome in outcomes {
        let compiled = match &outcome.compiled {
            Ok(file) => format!("так ({})", file.display()),
            Err(e) => format!("НІ — {e}"),
        };

        let verb = if outcome.draws {
            "намалював:    "
        } else {
            "wgpu прийняв: "
        };
        let accepted = match &outcome.accepted {
            Ok(()) => "так".to_string(),
            Err(e) => format!("НІ — {e}"),
        };

        println!("  {}", outcome.label);
        println!("    зкомпілювався: {compiled}");
        println!("    {verb} {accepted}");
    }

    println!();

    let drawing: Vec<&str> = outcomes
        .iter()
        .filter(|o| o.draws && o.accepted.is_ok())
        .map(|o| o.label)
        .collect();

    if drawing.is_empty() {
        println!("  Жоден шлях не малює. Розвилка ROADMAP P1: писати WGSL руками до M4.");
    } else {
        println!("  Малює: {}.", drawing.join(", "));
    }

    // Третій випадок існує саме заради цього речення: він відрізняє «SPIR-V
    // непридатний» від «SPIR-V придатний, крім однієї конструкції», а це
    // різні висновки.
    if let Some(control) = outcomes.iter().find(|o| !o.draws) {
        println!();
        match &control.accepted {
            Ok(()) => println!(
                "  Той самий шейдер без SV_VertexID wgpu приймає. Отже ламається \n  \
                 не SPIR-V як шлях, а конкретна capability."
            ),
            Err(_) => println!(
                "  Без SV_VertexID SPIR-V теж не приймається — річ не в цій \n  \
                 конструкції, а в шляху загалом."
            ),
        }
    }
}
