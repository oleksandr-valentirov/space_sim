//! P1 reconnaissance: does Slang's output reach wgpu (ROADMAP, stage E).
//!
//! The question is not "does it compile". It almost always compiles; it breaks
//! at the seam. So the probe walks the whole chain and stops only at the last
//! step:
//!
//!   1. `slangc` turns one `.slang` into WGSL and into SPIR-V;
//!   2. wgpu accepts each as a shader module;
//!   3. a pipeline is built from it;
//!   4. it **draws** a triangle into a 64x64 texture;
//!   5. the pixels are read back and compared against what should have come
//!      out.
//!
//! Step 5 is the point. A module that was created and drew nothing is a passed
//! check and a broken renderer; that is exactly what mismatched semantics or
//! location order looks like. So the colour is checked at three points rather
//! than the absence of an error.
//!
//! Two routes deliberately, because ROADMAP P1 says SPIR-V "works only in some
//! configurations" and we need to know which:
//!
//!   WGSL     `slangc -target wgsl`  -- naga parses it as ordinary text;
//!   SPIR-V   `slangc -target spirv` -- naga parses a binary (feature
//!                                     `spirv`).
//!
//! Run from the repository root:
//!
//!     sh scripts/fetch_slang.sh     once
//!     cargo run -p slang-probe

use std::path::{Path, PathBuf};
use std::process::Command;

const SHADERS: &str = "tools/slang-probe/shaders";

/// What exactly is checked. `draws` separates two different questions:
/// "reaches the picture" and "is accepted at all". The third case cannot draw
/// -- it needs vertex buffers -- but that is not what it is about.
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
        label: "SPIR-V without SV_VertexID",
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

/// The points where the result is checked, and which channel should dominate
/// at each.
///
/// The first version of these points sat exactly on the triangle's vertices --
/// and the check failed on a perfectly correct render: the rasteriser
/// legitimately does not fill a pixel exactly on a vertex. So the points are
/// moved inwards.
///
/// The corners matter as much as the interior: had the triangle covered the
/// whole screen or flipped, a "something was drawn" check would still
/// pass.
const INSIDE: &[(&str, u32, u32, usize)] = &[
    ("apex is red", SIZE / 2, SIZE / 3, 0),
    ("bottom left is green", SIZE / 3, (SIZE * 5) / 7, 1),
    ("bottom right is blue", (SIZE * 2) / 3, (SIZE * 5) / 7, 2),
];

const BACKGROUND: &[(&str, u32, u32)] = &[
    ("top left corner", 1, 1),
    ("top right corner", SIZE - 2, 1),
    ("bottom left corner", 1, SIZE - 2),
];

/// By how much the expected channel must dominate the others.
///
/// Dominance is checked rather than closeness to a pure colour: the triangle
/// interpolates vertex colours, so inside they are mixed, and any "looks red"
/// threshold would be tuned to specific coordinates.
const DOMINANCE: u8 = 60;

struct Outcome {
    label: &'static str,
    draws: bool,
    compiled: Result<PathBuf, String>,
    accepted: Result<(), String>,
}

fn main() {
    let version = std::fs::read_to_string(Path::new(SLANGC).parent().unwrap().join("../VERSION"))
        .unwrap_or_else(|_| "unknown".into());

    if !Path::new(SLANGC).exists() {
        eprintln!("missing {SLANGC}");
        eprintln!("  first: sh scripts/fetch_slang.sh");
        eprintln!("  run from the repository root");
        std::process::exit(1);
    }

    println!("Slang {}", version.trim());
    println!("shaders: {SHADERS}/\n");

    let (device, queue, adapter) = match open_device() {
        Some(triple) => triple,
        None => {
            eprintln!("no adapter to create a device with");
            std::process::exit(1);
        }
    };

    let info = adapter.get_info();
    println!(
        "adapter: {:?} -- {} ({:?})\n",
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
            Err(e) => Err(format!("did not reach loading: {e}")),
        };

        outcomes.push(Outcome {
            label: case.label,
            draws: case.draws,
            compiled,
            accepted,
        });
    }

    report(&outcomes);

    // Only NO route working counts as failure: that is the case ROADMAP P1
    // has the "write WGSL by hand until M4" fork for. One working route is a
    // successful reconnaissance, not half of one.
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
        .map_err(|e| format!("cannot run slangc: {e}"))?;

    if !result.status.success() {
        return Err(format!(
            "slangc -target {} returned {}:\n{}",
            case.target,
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }

    Ok(out)
}

fn load(device: &wgpu::Device, target: &str, file: &Path) -> Result<wgpu::ShaderModule, String> {
    // A shader parse error in wgpu arrives through the device error handler,
    // not as a Result. Without this harness a broken module would panic
    // somewhere later and the probe would report the wrong place.
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
        Some(error) => Err(format!("wgpu did not accept the module: {error}")),
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
        // Push constants are called immediates in wgpu 30; we do not need
        // them.
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
        return Err(format!("the pipeline did not build: {error}"));
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

    // 64 pixels of 4 bytes is exactly 256, so the row alignment requirement
    // is met without padding. That is why the size is this and not 100x100.
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
                    // Black background: the corner checks rest on it.
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
        .map_err(|e| format!("gave up waiting for the GPU: {e}"))?;

    let data = slice
        .get_mapped_range()
        .map_err(|e| format!("the buffer did not map: {e}"))?;
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
                "{label}: channel {channel} does not dominate, got {got:?}"
            ));
        }
    }

    for (label, x, y) in BACKGROUND {
        let got = pixel(*x, *y);
        if got != [0, 0, 0] {
            problems.push(format!("{label}: should have stayed background, got {got:?}"));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

fn report(outcomes: &[Outcome]) {
    println!("Result\n");

    for outcome in outcomes {
        let compiled = match &outcome.compiled {
            Ok(file) => format!("yes ({})", file.display()),
            Err(e) => format!("NO -- {e}"),
        };

        let verb = if outcome.draws {
            "drew:         "
        } else {
            "wgpu accepted:"
        };
        let accepted = match &outcome.accepted {
            Ok(()) => "yes".to_string(),
            Err(e) => format!("NO -- {e}"),
        };

        println!("  {}", outcome.label);
        println!("    compiled: {compiled}");
        println!("    {verb} {accepted}");
    }

    println!();

    let drawing: Vec<&str> = outcomes
        .iter()
        .filter(|o| o.draws && o.accepted.is_ok())
        .map(|o| o.label)
        .collect();

    if drawing.is_empty() {
        println!("  No route draws. ROADMAP P1 fork: write WGSL by hand until M4.");
    } else {
        println!("  Draws: {}.", drawing.join(", "));
    }

    // The third case exists for exactly this sentence: it separates "SPIR-V
    // is unusable" from "SPIR-V is usable except for one construct", and those
    // are different conclusions.
    if let Some(control) = outcomes.iter().find(|o| !o.draws) {
        println!();
        match &control.accepted {
            Ok(()) => println!(
                "  wgpu accepts the same shader without SV_VertexID. So what \n  \
                 breaks is not SPIR-V as a route but one specific capability."
            ),
            Err(_) => println!(
                "  SPIR-V is rejected without SV_VertexID too -- the problem \n  \
                 is the route in general, not this construct."
            ),
        }
    }
}
