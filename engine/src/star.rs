//! The star background pass (ROADMAP, stage Z, Z3).
//!
//! ## Where it goes in the frame, and why there
//!
//! First thing in the **farthest** depth range, before the sky. Two facts
//! force that and neither is obvious from the outside: the ranges do not share
//! a depth buffer -- each clears it and the order arranges them -- and the sky
//! adds its light to whatever is already in the target. Stars drawn after the
//! sky would cover the air instead of shining through it, and stars drawn in a
//! nearer range would sit in front of the planet.
//!
//! ## What it does not do
//!
//! **Colour.** The asset carries a B-V index for every star and this pass
//! ignores it: every star is white. That is a step of its own with an oracle
//! of its own (Sirius blue-white against Betelgeuse orange), and folding it in
//! here would mean the brightness oracle below could no longer say which of
//! the two conversions it had caught.
//!
//! **Twinkling, glare, spikes.** All three are camera artefacts rather than
//! sky, and a spaceship's camera has none of them.

use crate::gpu::Gpu;
use crate::stars::Catalogue;

/// The radiance of a magnitude-zero star, linear, in the frame's own units.
///
/// **Chosen rather than derived, and it has to be said plainly.** A physical
/// conversion would need the pixel's solid angle and the camera's aperture,
/// and the engine models neither: a star is a point source spread over a fixed
/// two pixels by the shader, not an image of a disc. So this is the number
/// that puts the naked-eye range where it belongs on the scale, and here is
/// the arithmetic it was picked by, at exposure 1.0:
///
/// * magnitude 0 -> 0.5 linear -> byte 182 after sRGB, bright and not clipped;
/// * magnitude 6.5, the catalogue's faint end -> 0.00126 -> byte 4, which is
///   the last value distinguishable from black;
/// * Sirius at -1.46 -> 1.92, above the tonemapper's knee, so the brightest
///   star in the sky is the one that saturates. That is the correct end of the
///   scale to lose.
pub const MAGNITUDE_ZERO_RADIANCE: f32 = 0.5;

/// The star's diameter on screen, pixels.
///
/// Two rather than one: a single-pixel star lands between pixels as often as
/// on one, and the sky would flicker as the camera turns. Two pixels with a
/// smooth falloff move continuously instead.
pub const STAR_PIXELS: f32 = 2.0;

/// Bytes per star in the storage buffer: a direction and a flux.
const STAR_BYTES: u64 = 16;

/// Bytes in the view uniform: four vectors.
const VIEW_BYTES: u64 = 64;

/// Flux relative to a magnitude-zero star.
///
/// The definition of the magnitude scale, and the only place it appears: five
/// magnitudes are a factor of a hundred, so one magnitude is the fifth root of
/// a hundred and the exponent is `-0.4 m`.
pub fn flux(magnitude: f32) -> f32 {
    10.0f32.powf(-0.4 * magnitude)
}

pub struct Stars {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    view: wgpu::Buffer,
    /// The catalogue on the GPU, and how many stars are in it. `None` until
    /// one is loaded -- a scene without a sky asset draws no stars and says
    /// nothing about it, exactly as a body without a tileset draws smooth.
    loaded: Option<(wgpu::BindGroup, u32)>,
}

impl Stars {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Stars {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("star"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/star.wgsl").into()),
            });

        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("star"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("star"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });

        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("star"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex_star"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fragment_star"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // Addition, like the sky and for the same reason: a
                        // star adds light, it does not replace what is behind
                        // it. It also makes two stars inside one pixel add up,
                        // which is what a double star does.
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
                // No depth written and the test always passes -- the same
                // state as the sky. The stars are at infinity; there is
                // nothing to argue with them about, and everything drawn
                // afterwards lands on top.
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

        let view = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star view"),
            size: VIEW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Stars {
            pipeline,
            layout,
            view,
            loaded: None,
        }
    }

    /// Put a catalogue on the GPU. Replaces whatever was there.
    ///
    /// The magnitude becomes a flux here, on the CPU, once per load: the
    /// asset keeps magnitudes so that it does not depend on an exposure, and
    /// the shader wants a ratio so that it does not need an exponent. This is
    /// the one place that knows both.
    pub fn load(&mut self, gpu: &Gpu, catalogue: &Catalogue) {
        let mut bytes = Vec::with_capacity(catalogue.stars.len() * STAR_BYTES as usize);
        for star in &catalogue.stars {
            for value in star.dir {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&flux(star.magnitude).to_le_bytes());
        }
        // An empty buffer cannot be bound, and an empty catalogue is a
        // legitimate state (a reader that found no star returns an error, but
        // a caller may hand us one it built itself).
        if bytes.is_empty() {
            self.loaded = None;
            return;
        }

        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stars"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buffer, 0, &bytes);

        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stars"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.view.as_entire_binding(),
                },
            ],
        });

        self.loaded = Some((group, catalogue.stars.len() as u32));
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    /// The camera for this frame.
    ///
    /// The basis is packed exactly as `sky` packs it -- direction in `xyz`,
    /// the half-angle tangent in `w` -- because the shader inverts what the
    /// sky's `pixel_ray` does, and two conventions for one basis is how a sign
    /// gets lost.
    /// Eight arguments, and a struct to hold them would be the same list with
    /// a name on it: each is an independent part of one camera, there is one
    /// caller, and `sky::View` cannot be reused -- it also carries the eye and
    /// the Sun, which this pass has no use for. The same call the engine's
    /// probe makes for the same reason.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        gpu: &Gpu,
        right: [f32; 3],
        up: [f32; 3],
        forward: [f32; 3],
        tan_half: [f32; 2],
        width: u32,
        height: u32,
    ) {
        let mut bytes = Vec::with_capacity(VIEW_BYTES as usize);
        let mut push = |values: [f32; 4]| {
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        };
        push([right[0], right[1], right[2], tan_half[0]]);
        push([up[0], up[1], up[2], tan_half[1]]);
        push([forward[0], forward[1], forward[2], 0.0]);
        push([
            STAR_PIXELS,
            width.max(1) as f32,
            height.max(1) as f32,
            MAGNITUDE_ZERO_RADIANCE,
        ]);
        gpu.queue.write_buffer(&self.view, 0, &bytes);
    }

    /// Draw into someone else's pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((group, count)) = &self.loaded else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        // Six vertices per star, no vertex buffer: rule 5 of stage R.
        pass.draw(0..count * 6, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Five magnitudes are a factor of a hundred -- the definition of the
    /// scale, and the only thing `flux` has to get right.
    #[test]
    fn five_magnitudes_are_a_hundredfold() {
        for start in [-2.0, 0.0, 3.5] {
            let ratio = flux(start) / flux(start + 5.0);
            assert!(
                (ratio - 100.0).abs() / 100.0 < 1.0e-5,
                "five magnitudes gave a factor of {ratio}"
            );
        }
        assert!(
            (flux(0.0) - 1.0).abs() < 1.0e-6,
            "magnitude zero is the unit"
        );
    }

    /// The scale runs the way the sky does: smaller magnitude, brighter star.
    #[test]
    fn a_smaller_magnitude_is_a_brighter_star() {
        assert!(flux(-1.46) > flux(0.0), "Sirius should outshine Vega");
        assert!(flux(0.0) > flux(6.5), "Vega should outshine the faint end");
    }
}
