//! Sky and air on the GPU: the Hillaire 2020 tables (ROADMAP-ATMOSPHERE.md).
//!
//! What lives here are the **tables**, not the picture. The split is not
//! cosmetic: the tables differ in how often they have to be computed, and that
//! is what decides where they stand in the frame (rule 5 of stage S):
//!
//! | table | depends on | how often |
//! |---|---|---|
//! | transmittance | air parameters only | once per parameter set |
//! | multiple scattering | air parameters only | once per parameter set |
//! | sky-view | camera position + direction to the Sun | once per frame |
//! | aerial perspective | the camera frustum | once per frame, and not always |
//!
//! The remaining rows will appear together with their steps; CLAUDE.md forbids
//! introducing them in advance outright.
//!
//! ## Two bind groups: what is read and what is written
//!
//! Group 0 is what a pass reads, group 1 what it writes. The split is forced:
//! the transmittance table is written by one pass and read by all the following
//! ones, and a single bind group holding the same texture both for writing and
//! for reading is forbidden -- wgpu sees a race in it regardless of what the
//! shader does. So the transmittance pass gets a **trimmed** layout for group 0,
//! without the table itself, and that is not a trick: a layout is required to
//! cover what the entry point reads, not everything the module has.
//!
//! ## Why [`Sky::ensure`] submits work itself rather than into someone's encoder
//!
//! Because this is not the frame's work. The transmittance table is recomputed
//! when the air parameters change, that is practically never; threaded through
//! the frame's encoder it would look per-frame, and the first person who comes
//! to optimise it will spend a day. The tables that **really** are computed
//! every frame go into the frame encoder -- and the difference between them
//! becomes visible from the code.

use crate::atmosphere;
use crate::gpu::Gpu;
use crate::scene::Atmosphere;

/// WGSL generated from `shaders/sky.slang` (`scripts/build_shaders.sh`).
const SKY_WGSL: &str = include_str!("../shaders/sky.wgsl");

/// The table format. Half-float: transmittance lies in `[0, 1]`, and eleven
/// significant bits are plenty there -- measured by the S2 test, which compares
/// the table against an `f64` oracle.
const LUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// How many bytes `AirParams` takes in the shader: four `float4`s.
const AIR_BYTES: u64 = 80;

/// How many bytes `ViewParams` takes in the shader: six `float4`s.
const VIEW_BYTES: u64 = 96;

/// How many bytes `PassParams` takes: one `float4`.
const PASS_BYTES: u64 = 16;

/// The stride between the `PassParams` of neighbouring depth ranges in the
/// buffer.
///
/// 256 bytes is the dynamic-offset alignment wgpu requires on all three
/// targets. The same number and for the same reason as `frame::PASS_STRIDE`.
const PASS_STRIDE: u64 = 256;

/// How many times brighter the sky becomes before being written into the frame.
///
/// **A constant rather than auto-exposure**, and that is a decision of the
/// stage: auto-exposure concerns the whole scene, not the air, and without a
/// ship in the frame there is nothing to measure it on (ROADMAP-ATMOSPHERE.md,
/// "чого етап S свідомо не робить").
///
/// The number is measured rather than eyeballed: the zenith radiance at noon
/// comes out at 0.048 per unit of solar illuminance, and a factor of 8 puts it
/// at 0.38 -- a daytime sky that does not hit one even near the horizon. It will
/// have to be revised when auto-exposure appears, and in that same step.
pub const EXPOSURE: f32 = 8.0;

/// The group size in `transmittance_main` -- the same as in
/// `[numthreads(8, 8, 1)]`.
const GROUP: u32 = 8;

/// Where the camera stands relative to the body with air -- everything the sky
/// pass knows about it.
///
/// Assembled on the CPU in `f64` and narrowed once: subtracting the body centre
/// from the eye is the same camera-relative as everywhere (F4). The screen axes
/// are already unit vectors, the tangents of the half field of view come
/// alongside, and together they give a pixel ray without a single inverse
/// matrix.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The camera relative to the body centre, metres, world axes.
    pub eye: [f64; 3],
    /// The direction TO the Sun, world axes, unit length.
    pub sun: [f32; 3],
    /// The screen axes in world coordinates.
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    /// Tangents of the half field of view: horizontal and vertical.
    pub tan_half: [f32; 2],
}

impl View {
    /// The camera's distance from the body centre.
    pub fn radius(&self) -> f64 {
        let e = self.eye;
        (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt()
    }

    /// The cosine of the Sun's zenith angle at the camera.
    pub fn sun_zenith_cos(&self) -> f64 {
        let r = self.radius().max(1.0);
        let e = self.eye;
        (e[0] * f64::from(self.sun[0])
            + e[1] * f64::from(self.sun[1])
            + e[2] * f64::from(self.sun[2]))
            / r
    }
}

pub struct Sky {
    transmittance_pipeline: wgpu::ComputePipeline,
    multiscatter_pipeline: wgpu::ComputePipeline,
    skyview_pipeline: wgpu::ComputePipeline,
    /// Two pipelines rather than a branch in the shader: a camera inside the
    /// air reads the table, a camera outside it marches. The CPU makes the
    /// choice -- the same decision as with a smooth body and a body with terrain
    /// (R5c).
    inside_pipeline: wgpu::RenderPipeline,
    outside_pipeline: wgpu::RenderPipeline,
    /// Aerial perspective (S5): the volume in compute, composition in two
    /// draws.
    aerial_pipeline: wgpu::ComputePipeline,
    multiply_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    write_aerial: wgpu::BindGroup,
    /// The composition group. Recreated together with the depth buffer -- it
    /// holds a reference to it.
    composite: Option<Composite>,
    composite_layout: wgpu::BindGroupLayout,
    pass_buffer: wgpu::Buffer,
    /// Group 0 for drawing: both constant tables, the sky-view table and the
    /// frame parameters, all visible to the fragment stage.
    read_draw: wgpu::BindGroup,

    /// Dims the star background by the air (Z4). Reads the transmittance
    /// table, so it uses `read_draw` like the sky itself.
    star_extinction: wgpu::RenderPipeline,

    /// Group 0 without the transmittance table itself -- for the pass that
    /// writes it.
    read_min: wgpu::BindGroup,
    /// Group 0 with the transmittance table -- for everyone who reads it.
    read_full: wgpu::BindGroup,
    /// Group 0 with both constant tables -- for whoever computes the sky every
    /// frame.
    read_frame: wgpu::BindGroup,
    /// Group 1 of each pass: exactly what it writes.
    write_transmittance: wgpu::BindGroup,
    write_multiscatter: wgpu::BindGroup,
    write_skyview: wgpu::BindGroup,

    air_buffer: wgpu::Buffer,
    view_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,

    transmittance: wgpu::Texture,
    multiscatter: wgpu::Texture,
    skyview: wgpu::Texture,
    skyview_view: wgpu::TextureView,
    aerial_inscatter_view: wgpu::TextureView,
    aerial_transmittance_view: wgpu::TextureView,

    /// The parameters the tables are already computed for: the air itself and
    /// the surface radius of the body it belongs to.
    ///
    /// The radius separately, because [`Atmosphere`] does not have it: only the
    /// upper bound is there. Two bodies with the same air and different radii
    /// are different atmospheres, and the key must see that.
    current: Option<(Atmosphere, f64, [u32; 3])>,
}

/// The composition group together with the depth-buffer size it was made for.
///
/// A separate struct because it is the only thing in [`Sky`] that depends on the
/// target size: depth is recreated when the window resizes, and the bind group
/// holds a reference to it.
struct Composite {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// One layout entry -- so that four almost identical layouts do not take up a
/// page.
fn storage_2d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: LUT_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

/// A table for reading: `float4`, bilinear filtering.
fn sampled_2d_for(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampled_2d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    sampled_2d_for(binding, wgpu::ShaderStages::COMPUTE)
}

/// A volume for writing.
fn storage_3d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: LUT_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D3,
        },
        count: None,
    }
}

/// A volume for reading, with three-dimensional filtering.
fn sampled_3d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    }
}

/// The aerial-perspective volume: 32x32x32.
fn aerial_texture(gpu: &Gpu, label: &str) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: atmosphere::AERIAL_XY,
            height: atmosphere::AERIAL_XY,
            depth_or_array_layers: atmosphere::AERIAL_Z,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: LUT_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// A table texture: written by compute, read by shaders, read back by a check.
///
/// `COPY_SRC` is here for the oracle, and that is not hidden: the stage-S checks
/// compare the table against `engine::atmosphere`, and it can only be read from
/// here. The same reason as in `indirect_buffer` (R6b).
fn lut_texture(gpu: &Gpu, label: &str, width: u32, height: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LUT_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

impl Sky {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Sky {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sky"),
                source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
            });

        let air_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(AIR_BYTES),
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };

        let read_min_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read (no luts)"),
                    entries: &[air_entry, sampler_entry],
                });
        let read_full_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read"),
                    entries: &[air_entry, sampler_entry, sampled_2d(2)],
                });
        let view_entry = wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(VIEW_BYTES),
            },
            count: None,
        };
        let read_frame_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read (frame)"),
                    entries: &[
                        air_entry,
                        sampler_entry,
                        sampled_2d(2),
                        sampled_2d(3),
                        view_entry,
                    ],
                });
        let write_transmittance_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write transmittance"),
                    entries: &[storage_2d(0)],
                });
        let write_multiscatter_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write multiscatter"),
                    entries: &[storage_2d(1)],
                });
        // Drawing: the same group 0, but visible to the FRAGMENT stage and
        // with the sky-view table instead of the slot it is written into.
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let read_draw_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read (draw)"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..air_entry
                        },
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..sampler_entry
                        },
                        sampled_2d_for(2, fragment),
                        sampled_2d_for(3, fragment),
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..view_entry
                        },
                        sampled_2d_for(5, fragment),
                    ],
                });
        let write_skyview_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write skyview"),
                    entries: &[storage_2d(2)],
                });
        let write_aerial_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write aerial"),
                    entries: &[storage_3d(3), storage_3d(4)],
                });

        // Composition reads depth as a texture, both volumes and the range
        // parameters. It does not need the air at all: it computes nothing and
        // only picks from what is already computed, and the layout shows that --
        // there is no `air` here.
        let composite_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky composite"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..sampler_entry
                        },
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..view_entry
                        },
                        sampled_3d(6),
                        sampled_3d(7),
                        wgpu::BindGroupLayoutEntry {
                            binding: 8,
                            visibility: fragment,
                            ty: wgpu::BindingType::Texture {
                                // Depth is read with `textureLoad`, without
                                // filtering: a value halfway between two
                                // surfaces belongs to neither.
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 9,
                            visibility: fragment,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                // An offset per depth range rather than a
                                // buffer per range -- the same decision as for
                                // patches (R4a).
                                has_dynamic_offset: true,
                                min_binding_size: std::num::NonZeroU64::new(PASS_BYTES),
                            },
                            count: None,
                        },
                    ],
                });

        let compute = |label: &str,
                       read: &wgpu::BindGroupLayout,
                       write: &wgpu::BindGroupLayout,
                       entry: &str| {
            let layout = gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts: &[Some(read), Some(write)],
                    immediate_size: 0,
                });
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        let transmittance_pipeline = compute(
            "transmittance",
            &read_min_layout,
            &write_transmittance_layout,
            "transmittance_main",
        );
        let multiscatter_pipeline = compute(
            "multiscatter",
            &read_full_layout,
            &write_multiscatter_layout,
            "multiscatter_main",
        );
        let skyview_pipeline = compute(
            "skyview",
            &read_frame_layout,
            &write_skyview_layout,
            "skyview_main",
        );
        let aerial_pipeline = compute(
            "aerial",
            &read_frame_layout,
            &write_aerial_layout,
            "aerial_main",
        );

        // The sky pass draws a full-screen triangle with no vertex buffers and
        // no depth writes: it goes first in the farthest range, and everything
        // after it lands on top by the ordinary depth test.
        let draw_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky draw"),
                bind_group_layouts: &[Some(&read_draw_layout)],
                immediate_size: 0,
            });
        let draw = |label: &str, entry: &str| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&draw_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_sky"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            // **Addition, not replacement.** Air glows rather
                            // than covers: what is behind it stays visible.
                            // Replacement was visible immediately -- the night
                            // edge of the limb from orbit bit a black arc out of
                            // the background, because there is nothing to scatter
                            // there, and zero became a colour.
                            //
                            // The full composition is `background*T + L`, that is
                            // the background is also attenuated by the air. The
                            // second factor arrives together with aerial
                            // perspective (S5), and that is where it is needed:
                            // while there is nothing behind the sky but the clear
                            // colour, there is nothing to attenuate.
                            blend: Some(wgpu::BlendState {
                                color: wgpu::BlendComponent {
                                    src_factor: wgpu::BlendFactor::One,
                                    dst_factor: wgpu::BlendFactor::One,
                                    operation: wgpu::BlendOperation::Add,
                                },
                                alpha: wgpu::BlendComponent::REPLACE,
                            }),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: crate::depth::FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let inside_pipeline = draw("sky inside", "fragment_sky_inside");
        let outside_pipeline = draw("sky outside", "fragment_sky_outside");

        // Composition draws into the frame with no depth buffer at all: it
        // reads depth as a texture, and one texture cannot be a target and a
        // resource at the same time.
        let composite_pipeline_layout =
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("sky composite"),
                    bind_group_layouts: &[Some(&composite_layout)],
                    immediate_size: 0,
                });
        let composite_draw = |label: &str, entry: &str, blend: wgpu::BlendState| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&composite_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_sky"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::COLOR,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        // The star background dimmed by the air (Z4).
        //
        // Its own pipeline rather than one of the two helpers, because it
        // needs one thing from each: the multiply blend of a composition pass,
        // and the depth state of a drawing one. It runs **inside** the frame's
        // own pass, between the stars and the sky, and a pipeline with no
        // depth state cannot be used in a pass that has a depth attachment.
        let star_extinction_pipeline =
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("star extinction"),
                    layout: Some(&draw_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_sky"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fragment_star_extinction"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            // `dst * T`: the source is multiplied by zero, the
                            // target by the source. The same blend the aerial
                            // multiply uses, for the same reason.
                            blend: Some(wgpu::BlendState {
                                color: wgpu::BlendComponent {
                                    src_factor: wgpu::BlendFactor::Zero,
                                    dst_factor: wgpu::BlendFactor::Src,
                                    operation: wgpu::BlendOperation::Add,
                                },
                                alpha: wgpu::BlendComponent::REPLACE,
                            }),
                            write_mask: wgpu::ColorWrites::COLOR,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: crate::depth::FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                });

        // `dst * T`: the source is multiplied by zero, the target by the
        // source.
        let multiply_pipeline = composite_draw(
            "aerial multiply",
            "fragment_aerial_multiply",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::Src,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            },
        );
        // `dst + L`.
        let add_pipeline = composite_draw(
            "aerial add",
            "fragment_aerial_add",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            },
        );

        let air_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("air params"),
            size: AIR_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view params"),
            size: VIEW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pass_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("depth range params"),
            size: PASS_STRIDE * crate::frame::MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Clamp at the edges rather than repeat: a table is a function defined
        // on an interval, and continuing it cyclically past that interval would
        // mean reading the zenith instead of the horizon.
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky luts"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let transmittance = lut_texture(
            gpu,
            "transmittance lut",
            atmosphere::TRANSMITTANCE_WIDTH,
            atmosphere::TRANSMITTANCE_HEIGHT,
        );
        let multiscatter = lut_texture(
            gpu,
            "multiscatter lut",
            atmosphere::MULTISCATTER_SIZE,
            atmosphere::MULTISCATTER_SIZE,
        );
        let skyview = lut_texture(
            gpu,
            "skyview lut",
            atmosphere::SKYVIEW_WIDTH,
            atmosphere::SKYVIEW_HEIGHT,
        );
        let transmittance_view = transmittance.create_view(&wgpu::TextureViewDescriptor::default());
        let multiscatter_view = multiscatter.create_view(&wgpu::TextureViewDescriptor::default());
        let skyview_view = skyview.create_view(&wgpu::TextureViewDescriptor::default());
        let aerial_inscatter = aerial_texture(gpu, "aerial inscatter");
        let aerial_transmittance = aerial_texture(gpu, "aerial transmittance");
        let aerial_inscatter_view =
            aerial_inscatter.create_view(&wgpu::TextureViewDescriptor::default());
        let aerial_transmittance_view =
            aerial_transmittance.create_view(&wgpu::TextureViewDescriptor::default());

        let read_min = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read (no luts)"),
            layout: &read_min_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let read_full = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read"),
            layout: &read_full_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
            ],
        });
        let read_frame = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read (frame)"),
            layout: &read_frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&multiscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: view_buffer.as_entire_binding(),
                },
            ],
        });
        let write_transmittance = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write transmittance"),
            layout: &write_transmittance_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&transmittance_view),
            }],
        });
        let write_multiscatter = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write multiscatter"),
            layout: &write_multiscatter_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&multiscatter_view),
            }],
        });

        let read_draw = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read (draw)"),
            layout: &read_draw_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&multiscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: view_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&skyview_view),
                },
            ],
        });
        let write_skyview = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write skyview"),
            layout: &write_skyview_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&skyview_view),
            }],
        });

        let write_aerial = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write aerial"),
            layout: &write_aerial_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&aerial_inscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&aerial_transmittance_view),
                },
            ],
        });

        Sky {
            transmittance_pipeline,
            multiscatter_pipeline,
            skyview_pipeline,
            inside_pipeline,
            outside_pipeline,
            aerial_pipeline,
            multiply_pipeline,
            add_pipeline,
            write_aerial,
            composite: None,
            composite_layout,
            pass_buffer,
            read_draw,
            star_extinction: star_extinction_pipeline,
            read_min,
            read_full,
            read_frame,
            write_transmittance,
            write_multiscatter,
            write_skyview,
            air_buffer,
            view_buffer,
            sampler,
            transmittance,
            multiscatter,
            skyview,
            skyview_view,
            aerial_inscatter_view,
            aerial_transmittance_view,
            current: None,
        }
    }

    /// The tables for this air -- computed if they are not for it yet.
    ///
    /// Returns `true` if computing was actually needed. The value is needed not
    /// by the frame but by a check: "the tables are not recomputed every frame"
    /// is a claim one must be able to verify rather than merely write in a
    /// comment.
    ///
    /// The order of passes here is a data dependency: scattering reads
    /// transmittance. wgpu places the barrier between them itself, from resource
    /// usage; separate passes are needed only because within one pass the order
    /// of groups is not guaranteed.
    pub fn ensure(&mut self, gpu: &Gpu, air: &Atmosphere, bottom_m: f64, albedo: [f32; 3]) -> bool {
        // The albedo enters the key together with the air: it changes the
        // **tables**, not the frame, so a rebuild must happen exactly when it
        // changes. The comparison is bitwise -- this is an input, not the result
        // of a measurement.
        let key = (*air, bottom_m, albedo.map(f32::to_bits));
        if self.current == Some(key) {
            return false;
        }

        gpu.queue
            .write_buffer(&self.air_buffer, 0, &air_bytes(air, bottom_m, albedo));

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sky luts"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("transmittance"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.transmittance_pipeline);
            pass.set_bind_group(0, &self.read_min, &[]);
            pass.set_bind_group(1, &self.write_transmittance, &[]);
            pass.dispatch_workgroups(
                atmosphere::TRANSMITTANCE_WIDTH.div_ceil(GROUP),
                atmosphere::TRANSMITTANCE_HEIGHT.div_ceil(GROUP),
                1,
            );
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("multiscatter"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.multiscatter_pipeline);
            pass.set_bind_group(0, &self.read_full, &[]);
            pass.set_bind_group(1, &self.write_multiscatter, &[]);
            let groups = atmosphere::MULTISCATTER_SIZE.div_ceil(GROUP);
            pass.dispatch_workgroups(groups, groups, 1);
        }
        gpu.queue.submit([encoder.finish()]);

        self.current = Some(key);
        true
    }

    /// The sky for this camera -- **into someone else's encoder**, because this
    /// is the frame's work.
    ///
    /// The difference from [`Sky::ensure`] is the main one and visible from the
    /// signature: the constant tables submit their own work and almost never,
    /// while this goes where the frame's passes go, that is every frame. Whoever
    /// comes to optimise will see it from the code.
    pub fn prepare_view(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, view: &View) {
        // The depth of the aerial-perspective volume is computed here rather
        // than in the frame: it depends on the air, and only `Sky` knows the
        // air. The frame would have to carry a second copy of the same
        // formula.
        let span = match self.current {
            Some((air, bottom, _)) => atmosphere::aerial_span(&air, bottom, view.radius()),
            None => (0.0, 1.0),
        };
        gpu.queue
            .write_buffer(&self.view_buffer, 0, &view_bytes(view, span));

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("skyview"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.skyview_pipeline);
        pass.set_bind_group(0, &self.read_frame, &[]);
        pass.set_bind_group(1, &self.write_skyview, &[]);
        pass.dispatch_workgroups(
            atmosphere::SKYVIEW_WIDTH.div_ceil(GROUP),
            atmosphere::SKYVIEW_HEIGHT.div_ceil(GROUP),
            1,
        );
    }

    /// Draw the sky with a full-screen triangle.
    ///
    /// The caller decides `inside` -- it knows where the upper boundary of the
    /// air is; a pipeline choice on the CPU rather than a branch in the shader,
    /// for the same reason as with patches (R5c).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, inside: bool) {
        pass.set_pipeline(if inside {
            &self.inside_pipeline
        } else {
            &self.outside_pipeline
        });
        pass.set_bind_group(0, &self.read_draw, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Dim the star background by the air (Z4), into someone else's pass.
    ///
    /// Called **between** the stars and the sky, and only there. At that
    /// moment the target holds the stars and the clear colour and nothing
    /// else, so a fullscreen multiply attenuates exactly them. Called later it
    /// would dim the planet twice -- the composition already does that -- and
    /// called earlier it would dim nothing.
    pub fn dim_stars(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.star_extinction);
        pass.set_bind_group(0, &self.read_draw, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The aerial-perspective volume for this camera -- also into someone
    /// else's encoder.
    ///
    /// Called only when the air is genuinely visible in the frame: the caller
    /// ([`crate::frame::Frame`]) computes the condition, because it is about the
    /// frame rather than about the air.
    pub fn prepare_aerial(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aerial"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.aerial_pipeline);
        pass.set_bind_group(0, &self.read_frame, &[]);
        pass.set_bind_group(1, &self.write_aerial, &[]);
        // One thread per ray, not per texel: the layers of one column lie on
        // one ray and are computed in a single sweep along it.
        let groups = atmosphere::AERIAL_XY.div_ceil(GROUP);
        pass.dispatch_workgroups(groups, groups, 1);
    }

    /// The composition group for this depth buffer -- created if the size
    /// changed.
    ///
    /// Apart from the other groups precisely because it is the only one that
    /// depends on the target size: depth is recreated when the window resizes,
    /// and the group holds a reference to it.
    pub fn bind_depth(&mut self, gpu: &Gpu, depth: &wgpu::TextureView, width: u32, height: u32) {
        if let Some(composite) = &self.composite {
            if composite.width == width && composite.height == height {
                return;
            }
        }
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.view_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.aerial_inscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.aerial_transmittance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.pass_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(PASS_BYTES),
                    }),
                },
            ],
        });
        self.composite = Some(Composite {
            bind_group,
            width,
            height,
        });
    }

    /// Record how depth range `index` turns `z_ndc` back into metres.
    ///
    /// `a` and `b` are the coefficients of `z_ndc = -A + B/z`; where they come
    /// from is written in `crate::depth`, and the frame computes them: it alone
    /// knows the range boundaries.
    pub fn set_range(&self, gpu: &Gpu, index: usize, a: f64, b: f64) {
        let mut bytes = Vec::with_capacity(PASS_BYTES as usize);
        for value in [a as f32, b as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        gpu.queue
            .write_buffer(&self.pass_buffer, index as u64 * PASS_STRIDE, &bytes);
    }

    /// Composition: `frame * T + L` in two draws.
    ///
    /// Two rather than one: in a single pass this would need dual-source
    /// blending -- one more device feature and `@blend_src` in WGSL, which the
    /// Slang compiler does not emit. Two full-screen triangles cost less than a
    /// dependency on both.
    pub fn composite(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        let Some(composite) = &self.composite else {
            return;
        };
        let offset = (index as u64 * PASS_STRIDE) as u32;
        pass.set_pipeline(&self.multiply_pipeline);
        pass.set_bind_group(0, &composite.bind_group, &[offset]);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.add_pipeline);
        pass.set_bind_group(0, &composite.bind_group, &[offset]);
        pass.draw(0..3, 0..1);
    }

    /// A view of the sky-view table -- for whoever will draw the frame with it
    /// (S4b).
    pub fn skyview_view(&self) -> &wgpu::TextureView {
        &self.skyview_view
    }

    /// The sky-view table back into memory -- the S4 oracle.
    pub fn read_skyview(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        read_lut(
            gpu,
            &self.skyview,
            atmosphere::SKYVIEW_WIDTH,
            atmosphere::SKYVIEW_HEIGHT,
        )
    }

    /// The transmittance table back into memory -- the S2 oracle.
    pub fn read_transmittance(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        read_lut(
            gpu,
            &self.transmittance,
            atmosphere::TRANSMITTANCE_WIDTH,
            atmosphere::TRANSMITTANCE_HEIGHT,
        )
    }

    /// The multiple-scattering table back into memory -- the S3 oracle.
    ///
    /// RGB is `psi`, alpha is the largest channel of `f`, that is the number the
    /// convergence of the series depends on.
    pub fn read_multiscatter(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        read_lut(
            gpu,
            &self.multiscatter,
            atmosphere::MULTISCATTER_SIZE,
            atmosphere::MULTISCATTER_SIZE,
        )
    }
}

/// A table from the GPU back into memory.
///
/// This must not be called in a frame: there is a `poll(Wait)` here, that is a
/// full pipeline stall. It exists purely for checking, like
/// [`crate::frame::Frame::drawn_patches`].
///
/// Row-major, `[r, g, b, a]` per texel, already unpacked from half-float.
fn read_lut(
    gpu: &Gpu,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<[f32; 4]>, String> {
    // Eight bytes per texel; both 256 and 32 texels per row give a multiple of
    // 256, so the `copy_texture_to_buffer` alignment holds by itself and no
    // padding is needed. Asserted rather than assumed: the next table may turn
    // out to be a different width.
    let bytes_per_row = width * 8;
    assert_eq!(
        bytes_per_row % 256,
        0,
        "a row of the {width}x{height} table is not aligned to 256 bytes"
    );

    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lut readback"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lut readback"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);

    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| format!("waiting for the GPU failed: {e}"))?;
    let data = slice
        .get_mapped_range()
        .map_err(|e| format!("the buffer did not map: {e}"))?;

    let mut out = Vec::with_capacity((width * height) as usize);
    for texel in data.chunks_exact(8) {
        let mut rgba = [0.0f32; 4];
        for (channel, half) in rgba.iter_mut().zip(texel.chunks_exact(2)) {
            *channel = from_half(u16::from_le_bytes([half[0], half[1]]));
        }
        out.push(rgba);
    }
    drop(data);
    staging.unmap();
    Ok(out)
}

/// The air parameters in the `AirParams` layout from `sky.slang`.
///
/// Written out by hand for the same reason as `Uniforms::to_bytes` in the frame:
/// our `unsafe` lives only in `core-rs` (CLAUDE.md, invariant 1).
///
/// **The radii are narrowed to `f32` here, and this is not the narrowing people
/// fear.** The rule "world coordinates never in a float" (F4) concerns positions
/// where the camera is subtracted from a large number; the body radius is not
/// one of those -- the altitude above the surface is computed from it, and
/// `6.371e6` in `f32` has a step of 0.5 m, that is an error of sixteen
/// millionths of the scale height.
fn air_bytes(air: &Atmosphere, bottom_m: f64, albedo: [f32; 3]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(AIR_BYTES as usize);
    let mut push = |values: [f32; 4]| {
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    };
    push([
        air.rayleigh_scattering[0],
        air.rayleigh_scattering[1],
        air.rayleigh_scattering[2],
        air.rayleigh_height_m,
    ]);
    push([
        air.mie_scattering,
        air.mie_absorption,
        air.mie_height_m,
        air.mie_g,
    ]);
    push([
        air.ozone_absorption[0],
        air.ozone_absorption[1],
        air.ozone_absorption[2],
        0.0,
    ]);
    push([
        air.ozone_centre_m,
        air.ozone_width_m,
        bottom_m as f32,
        air.top_m as f32,
    ]);
    // The mean surface albedo under this sky (T7h); `w` is spare.
    push([albedo[0], albedo[1], albedo[2], 0.0]);
    bytes
}

/// The camera parameters in the `ViewParams` layout from `sky.slang`.
fn view_bytes(view: &View, aerial_span: (f64, f64)) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(VIEW_BYTES as usize);
    let mut push = |values: [f32; 4]| {
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    };
    push([
        view.radius() as f32,
        view.sun_zenith_cos() as f32,
        EXPOSURE,
        aerial_span.1 as f32,
    ]);
    push([
        view.eye[0] as f32,
        view.eye[1] as f32,
        view.eye[2] as f32,
        aerial_span.0 as f32,
    ]);
    push([view.sun[0], view.sun[1], view.sun[2], 0.0]);
    push([
        view.right[0],
        view.right[1],
        view.right[2],
        view.tan_half[0],
    ]);
    push([view.up[0], view.up[1], view.up[2], view.tan_half[1]]);
    push([view.forward[0], view.forward[1], view.forward[2], 0.0]);
    bytes
}

/// Half-float into `f32`.
///
/// Ten lines instead of a dependency: `half` is already in the tree
/// transitively, but it would become a direct dependency of the engine for the
/// sake of one function needed only by a check. The IEEE 754 binary16 format is
/// covered in full -- sign, five exponent bits with a bias of 15, ten mantissa
/// bits.
fn from_half(bits: u16) -> f32 {
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;
    let magnitude = match exponent {
        // Zero and subnormals: the value is `mantissa * 2^-24`.
        0 => f32::from(mantissa) * (1.0 / 16_777_216.0),
        // Infinity and NaN cannot appear in a table, but silently turning them
        // into a finite number would hide a bug.
        0x1f if mantissa == 0 => f32::INFINITY,
        0x1f => f32::NAN,
        _ => f32::from_bits((u32::from(exponent) + (127 - 15)) << 23 | u32::from(mantissa) << 13),
    };
    if bits & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half-float unpacking -- on numbers that are easy to check by hand.
    #[test]
    fn half_floats_unpack_to_the_numbers_they_encode() {
        assert_eq!(from_half(0x0000), 0.0);
        assert_eq!(from_half(0x3c00), 1.0);
        assert_eq!(from_half(0x4000), 2.0);
        assert_eq!(from_half(0xc000), -2.0);
        assert_eq!(from_half(0x3800), 0.5);
        // The smallest normal: 2^-14.
        assert_eq!(from_half(0x0400), 2.0f32.powi(-14));
        // The largest subnormal: (1023/1024)*2^-14.
        assert!((from_half(0x03ff) - 1023.0 / 1024.0 * 2.0f32.powi(-14)).abs() < 1.0e-12);
        assert!(from_half(0x7c00).is_infinite());
        assert!(from_half(0x7e00).is_nan());
    }

    /// The `AirParams` layout -- the same as in the shader: twenty numbers,
    /// each in its place.
    #[test]
    fn the_air_params_land_where_the_shader_reads_them() {
        let air = Atmosphere::EARTH;
        // The albedo differs per channel on purpose: equal values would not
        // tell swapped components apart.
        let bytes = air_bytes(&air, 6_371_000.0, [0.11, 0.22, 0.33]);
        assert_eq!(bytes.len() as u64, AIR_BYTES);

        let word = |k: usize| f32::from_le_bytes(bytes[k * 4..k * 4 + 4].try_into().unwrap());
        assert_eq!(word(0), air.rayleigh_scattering[0]);
        assert_eq!(word(3), air.rayleigh_height_m);
        assert_eq!(word(4), air.mie_scattering);
        assert_eq!(word(7), air.mie_g);
        assert_eq!(word(8), air.ozone_absorption[0]);
        assert_eq!(word(12), air.ozone_centre_m);
        assert_eq!(word(14), 6_371_000.0);
        assert_eq!(word(15), air.top_m as f32);
        assert_eq!(word(16), 0.11);
        assert_eq!(word(17), 0.22);
        assert_eq!(word(18), 0.33);
    }
}
