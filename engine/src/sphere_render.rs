//! Пайплайн сфери: справжні вершинний та індексний буфери, reversed-Z
//! (ROADMAP F5).
//!
//! На відміну від `depth_probe` (геометрія рахується в шейдері з
//! `SV_VertexID`), тут вершини — це `engine::sphere::Mesh`. Позиції
//! перераховуються в `f32` camera-relative щокадру ([`crate::camera`]) і
//! йдуть в окремий буфер від нормалей: нормалі від камери не залежать,
//! перезавантажувати їх щоразу нема сенсу.

use crate::camera::Camera;
use crate::depth;
use crate::gpu::Gpu;
use crate::shot::Shot;
use crate::sphere::Mesh;

const SPHERE_WGSL: &str = include_str!("../shaders/sphere.wgsl");

const FOV_Y: f64 = std::f64::consts::PI / 3.0;

#[derive(Clone, Copy)]
struct Uniforms {
    projection: depth::Matrix,
    light_dir: [f32; 4],
    colour: [f32; 4],
}

impl Uniforms {
    /// Розкладка вручну — та сама причина, що в `depth_probe::Params`
    /// (CLAUDE.md, інваріант 1: наш `unsafe` живе лише в `core-rs`, тут
    /// його й не треба).
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(96);
        for column in self.projection {
            for value in column {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in self.light_dir.iter().chain(self.colour.iter()) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

pub struct Params {
    pub near: f64,
    pub light_dir: [f32; 3],
    pub colour: [f32; 4],
}

/// Малює `mesh` з погляду `camera` й читає кадр назад.
///
/// Пайплайн і буфери створюються заново щовиклику — той самий вибір, що в
/// `depth_probe::render_quads`: цей шлях перевіряє коректність, не частоту
/// кадрів (перевимір продуктивності — окремий крок після F5).
pub fn render(
    gpu: &Gpu,
    width: u32,
    height: u32,
    camera: &Camera,
    mesh: &Mesh,
    params: &Params,
) -> Result<Shot, String> {
    let aspect = f64::from(width) / f64::from(height);
    let projection = depth::reversed_infinite(FOV_Y, aspect, params.near);

    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sphere"),
            source: wgpu::ShaderSource::Wgsl(SPHERE_WGSL.into()),
        });

    let bind_layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sphere"),
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
            label: Some("sphere"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sphere"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex_main"),
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
                // Без відсікання граней: коректність тримається на тесті
                // глибини (сфера опукла, найближча поверхня завжди виграє),
                // а не на порядку обходу вершин. Той самий вибір, що в
                // depth_quad — і та сама причина: одна менш переконлива
                // умова.
                cull_mode: None,
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
        label: Some("sphere colour"),
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
        label: Some("sphere depth"),
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

    // Camera-relative щокадру: віднімання й поворот у double, звуження —
    // останній крок (ROADMAP F4, `camera::Camera::relative`).
    let mut position_bytes = Vec::with_capacity(mesh.positions.len() * 12);
    for &p in &mesh.positions {
        let rel = camera.relative(p);
        position_bytes.extend_from_slice(&rel[0].to_le_bytes());
        position_bytes.extend_from_slice(&rel[1].to_le_bytes());
        position_bytes.extend_from_slice(&rel[2].to_le_bytes());
    }

    let position_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sphere positions"),
        size: position_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&position_buffer, 0, &position_bytes);

    let mut normal_bytes = Vec::with_capacity(mesh.normals.len() * 12);
    for n in &mesh.normals {
        normal_bytes.extend_from_slice(&n[0].to_le_bytes());
        normal_bytes.extend_from_slice(&n[1].to_le_bytes());
        normal_bytes.extend_from_slice(&n[2].to_le_bytes());
    }

    let normal_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sphere normals"),
        size: normal_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&normal_buffer, 0, &normal_bytes);

    let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    let index_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sphere indices"),
        size: index_bytes.len() as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&index_buffer, 0, &index_bytes);

    let uniforms = Uniforms {
        projection,
        light_dir: [
            params.light_dir[0],
            params.light_dir[1],
            params.light_dir[2],
            0.0,
        ],
        colour: params.colour,
    };
    let uniform_bytes = uniforms.to_bytes();
    let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sphere uniforms"),
        size: uniform_bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&uniform_buffer, 0, &uniform_bytes);

    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sphere"),
        layout: &bind_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sphere"),
        });

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sphere"),
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
        pass.set_vertex_buffer(0, position_buffer.slice(..));
        pass.set_vertex_buffer(1, normal_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
    }

    crate::shot::read_back(gpu, encoder, &colour_texture, width, height)
}
