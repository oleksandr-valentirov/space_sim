//! Замір роздільності буфера глибини (ROADMAP F3).
//!
//! Малює два чотирикутники на майже однаковій відстані й рахує, яку частку
//! кадру виграв ближчий. Це і є прямий тест замість «здається, нормально»:
//!
//!   1.0 — глибина роздільна, ближчий скрізь попереду;
//!   0.0 — не роздільна взагалі, виграв той, кого намалювали першим;
//!   між — z-fighting, тобто саме мерехтіння, за яке крок і відповідає.
//!
//! Проміжне значення тут — не «частково працює». Це найгірший випадок:
//! у русі такий кадр блимає.

use crate::depth;
use crate::gpu::Gpu;
use crate::shot::Shot;

const QUAD_WGSL: &str = include_str!("../shaders/depth_quad.wgsl");

/// Поле зору по вертикалі, радіани. 60°, як у типової гри.
const FOV_Y: f64 = std::f64::consts::PI / 3.0;

pub struct Setup {
    pub reversed: bool,
    pub near: f64,
    /// Відстань до дальнього з двох чотирикутників, метри.
    pub distance: f64,
    /// Наскільки ближчий ближче, метри.
    pub gap: f64,
}

pub struct Measured {
    /// Частка пікселів, де попереду опинився ближчий.
    pub near_wins: f64,
    pub shot: Shot,
}

/// Кольори двох поверхонь. Далекий червоний, ближчий зелений — щоб на
/// знімку було видно оком, що саме перемогло.
const FAR_COLOUR: [f32; 4] = [0.9, 0.1, 0.1, 1.0];
const NEAR_COLOUR: [f32; 4] = [0.1, 0.9, 0.1, 1.0];

#[derive(Clone, Copy)]
pub struct Params {
    pub projection: depth::Matrix,
    pub colour: [f32; 4],
    pub placement: [f32; 4],
}

impl Params {
    /// Розкладка вручну, без `bytemuck` і без `unsafe`.
    ///
    /// Інваріант 1 з CLAUDE.md: наш `unsafe` живе лише в `core-rs`. Тут
    /// це не жертва — двадцять чотири числа підряд, і порядок видно оком,
    /// а не виводиться з `#[repr(C)]` і вирівнювання.
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(96);
        for column in self.projection {
            for value in column {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in self.colour.iter().chain(self.placement.iter()) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

pub fn measure(gpu: &Gpu, width: u32, height: u32, setup: &Setup) -> Result<Measured, String> {
    let aspect = f64::from(width) / f64::from(height);

    let projection = if setup.reversed {
        depth::reversed_infinite(FOV_Y, aspect, setup.near)
    } else {
        // Далека площина навмисно щедра: якщо звичайна проєкція програє
        // навіть так, справа не в тому, що її погано налаштували.
        depth::conventional(FOV_Y, aspect, setup.near, setup.distance * 10.0)
    };

    let quad = |distance: f64, colour: [f32; 4]| Params {
        projection,
        colour,
        placement: [
            0.0,
            0.0,
            -distance as f32,
            // Удвічі більше за півекран на цій відстані: перекриття має
            // накривати кадр цілком, інакше мерехтіння на краю не видно.
            (2.0 * distance * (FOV_Y / 2.0).tan() * aspect.max(1.0)) as f32,
        ],
    };

    let far = quad(setup.distance, FAR_COLOUR);
    let near = quad(setup.distance - setup.gap, NEAR_COLOUR);

    render_quads(gpu, width, height, setup.reversed, &[far, near])
}

/// Малює задані чотирикутники й читає кадр назад.
///
/// Публічна, бо тим самим користується [`crate::camera_probe`]: там інше
/// питання, але та сама сцена — один шейдер, один пайплайн, різниця лише в
/// тому, які числа доїхали.
pub fn render_quads(
    gpu: &Gpu,
    width: u32,
    height: u32,
    reversed: bool,
    quads: &[Params],
) -> Result<Measured, String> {
    render_ranges(gpu, width, height, reversed, &[quads])
}

/// Те саме, але **проходами**: глибина очищається між ними, колір — ні
/// (ROADMAP-PLANETS.md, R4b).
///
/// Один прохід — це рівно те, що робив [`render_quads`], тож зонд F3 нічого
/// не втратив. Кілька проходів дають те, заради чого існують діапазони:
/// поверхні в різних проходах не змагаються за біти глибини взагалі, і
/// порядок між ними вирішує порядок проходів.
pub fn render_ranges(
    gpu: &Gpu,
    width: u32,
    height: u32,
    reversed: bool,
    ranges: &[&[Params]],
) -> Result<Measured, String> {
    let quads: Vec<Params> = ranges.iter().flat_map(|g| g.iter().copied()).collect();
    let quads = &quads[..];
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth quad"),
            source: wgpu::ShaderSource::Wgsl(QUAD_WGSL.into()),
        });

    let bind_layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("depth quad"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

    let layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("depth quad"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth quad"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::shot::FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                // Без відсікання граней: чотирикутник має бути видимий, як
                // його не поверни. Порядок вершин тут не несе інформації.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth::FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(if reversed {
                    depth::COMPARE
                } else {
                    depth::CONVENTIONAL_COMPARE
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

    let colour = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth probe colour"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: crate::shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let colour_view = colour.create_view(&wgpu::TextureViewDescriptor::default());

    let depth_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth probe depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: depth::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut groups = Vec::new();
    for (i, params) in quads.iter().enumerate() {
        let bytes = params.to_bytes();

        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("depth quad params"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buffer, 0, &bytes);

        groups.push((
            i,
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("depth quad"),
                layout: &bind_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            }),
        ));
    }

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("depth probe"),
        });

    let mut first_quad = 0usize;
    for (range, group_quads) in ranges.iter().enumerate() {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if range == 0 {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(if reversed {
                        depth::CLEAR
                    } else {
                        depth::CONVENTIONAL_CLEAR
                    }),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(&pipeline);

        // Дальній малюється ПЕРШИМ. Тоді «нуль зелених» означає рівно одне:
        // глибина не розрізнила поверхонь і виграв порядок малювання, а не
        // геометрія.
        for (_, group) in &groups[first_quad..first_quad + group_quads.len()] {
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..6, 0..1);
        }
        first_quad += group_quads.len();
    }

    let shot = crate::shot::read_back(gpu, encoder, &colour, width, height)?;

    let mut near_wins = 0u64;
    let total = u64::from(width) * u64::from(height);
    for y in 0..height {
        for x in 0..width {
            let pixel = shot.pixel(x, y);
            if pixel[1] > pixel[0] {
                near_wins += 1;
            }
        }
    }

    Ok(Measured {
        near_wins: near_wins as f64 / total as f64,
        shot,
    })
}
