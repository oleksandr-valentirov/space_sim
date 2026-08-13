//! Що саме малюється. Про вікно тут не знають нічого — лише про текстуру,
//! у яку писати, і про її формат.
//!
//! Формат у конструкторі, а не в `draw`, бо пайплайн прив'язаний до формату
//! цілі. Вікно й знімок мають різні формати, тож у них різні [`Frame`] —
//! і це та сама причина, з якої вони дають трохи різні пікселі: поверхня
//! вікна sRGB, ціль знімка лінійна. Кольори в шейдері однакові; те, що з
//! ними робить апаратура на записі, — ні.

/// Колір очищення. Не чорний навмисно: чорний кадр і кадр, якого не було,
/// виглядають однаково, і перевірка «щось намалювалось» на чорному нічого
/// не варта.
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.03,
    b: 0.08,
    a: 1.0,
};

/// Той самий колір у байтах — для звірки знімка (лінійна ціль).
pub const CLEAR_BYTES: [u8; 3] = [5, 8, 20];

/// WGSL, згенерований зі `shaders/triangle.slang`.
///
/// Вбудовується в бінарник, а не читається з диска: шейдер — частина
/// програми, а не ассет, який можна підмінити. Генерується
/// `scripts/build_shaders.sh` і комітиться (ROADMAP F2).
const TRIANGLE_WGSL: &str = include_str!("../shaders/triangle.wgsl");

pub struct Frame {
    pipeline: wgpu::RenderPipeline,
}

impl Frame {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Frame {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("triangle"),
            source: wgpu::ShaderSource::Wgsl(TRIANGLE_WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("triangle"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("triangle"),
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
                    format,
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

        Frame { pipeline }
    }

    /// Записує в `encoder` усе, що складає кадр.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(&self.pipeline);
        pass.draw(0..3, 0..1);
    }
}
