//! The trajectory pipeline: a line, with the frame transform on the GPU
//! (ROADMAP F6).
//!
//! The vertex data is geocentric (`vessel - earth`, `moon - earth`, both at
//! that sample's time t) -- not camera-relative in the usual sense, because
//! there is no camera here: only the vessel and the Earth-Moon system, while
//! Earth itself travels billions of metres heliocentrically over the 233-day
//! mission. The same principle as in `camera.rs`, tied to a different
//! origin.

use crate::depth;
use crate::gpu::Gpu;
use crate::shot::Shot;
use crate::trajectory::{self, Sample, MU};

const TRAJECTORY_WGSL: &str = include_str!("../shaders/trajectory.wgsl");

const FOV_Y: f64 = std::f64::consts::PI / 3.0;

#[derive(Clone, Copy)]
struct Uniforms {
    projection: depth::Matrix,
    mu: f32,
    _pad0: [f32; 3],
    view_offset: [f32; 3],
    _pad1: f32,
    colour: [f32; 4],
}

impl Uniforms {
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(112);
        for column in self.projection {
            for value in column {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&self.mu.to_le_bytes());
        for value in self._pad0 {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in self.view_offset {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self._pad1.to_le_bytes());
        for value in self.colour {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// The same viewing angle as the vertex shader: projection onto the sample's
/// (x, y, z) basis, then a swizzle (x, z, -y) -- the camera along "y", where
/// the orbit's spread is smallest.
fn view_axes(vessel: [f64; 3], moon: [f64; 3], z_axis: [f64; 3]) -> [f64; 3] {
    let d = moon;
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let x_axis = [d[0] / length, d[1] / length, d[2] / length];
    let y_axis = [
        z_axis[1] * x_axis[2] - z_axis[2] * x_axis[1],
        z_axis[2] * x_axis[0] - z_axis[0] * x_axis[2],
        z_axis[0] * x_axis[1] - z_axis[1] * x_axis[0],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let chosen = [
        dot(vessel, x_axis),
        dot(vessel, y_axis),
        dot(vessel, z_axis),
    ];
    [chosen[0], chosen[2], -chosen[1]]
}

/// The centre and radius of the bounding sphere for framing the camera -- the
/// same view the shader will produce, computed in Rust rather than guessed.
#[derive(Clone, Copy)]
pub struct Framing {
    pub centre: [f64; 3],
    pub radius: f64,
}

fn frame(points: impl Iterator<Item = [f64; 3]>) -> Framing {
    let points: Vec<[f64; 3]> = points.collect();
    let n = points.len() as f64;
    let mut centre = [0.0; 3];
    for p in &points {
        centre[0] += p[0] / n;
        centre[1] += p[1] / n;
        centre[2] += p[2] / n;
    }
    let radius = points
        .iter()
        .map(|p| {
            let d = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .fold(0.0f64, f64::max);
    Framing { centre, radius }
}

/// Framing for the geocentric (rotating = 0) view.
pub fn geocentric_framing(samples: &[Sample]) -> Framing {
    frame(samples.iter().map(|s| {
        let vessel = [
            s.vessel[0] - s.earth[0],
            s.vessel[1] - s.earth[1],
            s.vessel[2] - s.earth[2],
        ];
        let moon = [
            s.moon[0] - s.earth[0],
            s.moon[1] - s.earth[1],
            s.moon[2] - s.earth[2],
        ];
        view_axes(vessel, moon, s.z_axis)
    }))
}

/// Framing for the rotating (rotating = 1) view -- the same swizzle, but on
/// `rotating_position` instead of the raw geocentric vector.
pub fn rotating_framing(samples: &[Sample]) -> Framing {
    frame(samples.iter().map(|s| {
        let r = trajectory::rotating_position(s.vessel, s.earth, s.moon, s.z_axis);
        // rotating_position is already in the (x,y,z) basis -- swizzle with no
        // second projection.
        [r[0], r[2], -r[1]]
    }))
}

pub struct Params {
    pub rotating: bool,
    pub framing: Framing,
    pub colour: [f32; 4],
}

/// Draws `samples` as a `LineStrip` and reads the frame back.
pub fn render(
    gpu: &Gpu,
    width: u32,
    height: u32,
    samples: &[Sample],
    params: &Params,
) -> Result<Shot, String> {
    let aspect = f64::from(width) / f64::from(height);

    // The far edge is 2.5 radii from the centre, so the shape fits with room
    // to spare; `near` is a hundredth of the same radius, far smaller than the
    // line's closest approach to the camera.
    let backoff = params.framing.radius * 2.5;
    let near = (params.framing.radius * 0.01).max(1e-6);
    let projection = depth::reversed_infinite(FOV_Y, aspect, near);

    let view_offset = [
        -params.framing.centre[0] as f32,
        -params.framing.centre[1] as f32,
        -params.framing.centre[2] as f32 - backoff as f32,
    ];

    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("trajectory"),
            source: wgpu::ShaderSource::Wgsl(TRAJECTORY_WGSL.into()),
        });

    let bind_layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("trajectory"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
            label: Some("trajectory"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("trajectory"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(if params.rotating {
                    "vertex_rotating"
                } else {
                    "vertex_geocentric"
                }),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        }],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 1,
                        }],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 2,
                        }],
                    }),
                ],
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
                topology: wgpu::PrimitiveTopology::LineStrip,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth::FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(depth::COMPARE),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

    let colour_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("trajectory colour"),
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
    let colour_view = colour_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let depth_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("trajectory depth"),
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

    let push = |bytes: &mut Vec<u8>, v: [f64; 3]| {
        for c in v {
            bytes.extend_from_slice(&(c as f32).to_le_bytes());
        }
    };

    let mut vessel_bytes = Vec::with_capacity(samples.len() * 12);
    let mut moon_bytes = Vec::with_capacity(samples.len() * 12);
    let mut z_bytes = Vec::with_capacity(samples.len() * 12);
    for s in samples {
        // Geocentric: this sample's Earth is subtracted here rather than in
        // the shader -- the same principle as camera-relative (F4, F5), tied to
        // Earth-at-time-t.
        push(
            &mut vessel_bytes,
            [
                s.vessel[0] - s.earth[0],
                s.vessel[1] - s.earth[1],
                s.vessel[2] - s.earth[2],
            ],
        );
        push(
            &mut moon_bytes,
            [
                s.moon[0] - s.earth[0],
                s.moon[1] - s.earth[1],
                s.moon[2] - s.earth[2],
            ],
        );
        push(&mut z_bytes, s.z_axis);
    }

    let make_buffer = |label: &str, bytes: &[u8]| {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buffer, 0, bytes);
        buffer
    };
    let vessel_buffer = make_buffer("trajectory vessel", &vessel_bytes);
    let moon_buffer = make_buffer("trajectory moon", &moon_bytes);
    let z_buffer = make_buffer("trajectory z_axis", &z_bytes);

    let uniforms = Uniforms {
        projection,
        mu: MU as f32,
        _pad0: [0.0; 3],
        view_offset,
        _pad1: 0.0,
        colour: params.colour,
    };
    let uniform_bytes = uniforms.to_bytes();
    let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("trajectory uniforms"),
        size: uniform_bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&uniform_buffer, 0, &uniform_bytes);

    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("trajectory"),
        layout: &bind_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("trajectory"),
        });

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("trajectory"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(depth::CLEAR),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vessel_buffer.slice(..));
        pass.set_vertex_buffer(1, moon_buffer.slice(..));
        pass.set_vertex_buffer(2, z_buffer.slice(..));
        pass.draw(0..samples.len() as u32, 0..1);
    }

    crate::shot::read_back(gpu, encoder, &colour_texture, width, height)
}
