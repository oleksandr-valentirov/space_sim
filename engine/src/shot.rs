//! A frame into a PNG, with no window.
//!
//! Not a convenience. "The window opened and did not crash" is not a check of
//! the renderer: a black frame looks exactly like a correct one. A shot can be
//! looked at, compared pixel by pixel, and run in CI, where there is no window
//! at all.

use std::path::Path;

use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::scene::Scene;

/// The target format is **the one the window picks** (T5a).
///
/// WARNING: before T5a this was `Rgba8Unorm`, explained as "so the shot holds
/// exactly the bytes that were written". The explanation was consistent and
/// wrong: `window.rs` picks a surface by the `is_srgb()` filter, so the window
/// has been encoding gamma in hardware **since F1**. Two paths to one frame
/// showed different pictures, and the engine claimed the opposite all along.
///
/// Now the shot encodes too, through the same hardware mechanism. The
/// consequence every new oracle must know: **the ratio of two bytes is no
/// longer the ratio of two luminances**. Before dividing --
/// [`crate::srgb::to_linear`].
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Shot {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ]
    }

    pub fn write_png(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&self.pixels)
            .map_err(|e| e.to_string())
    }
}

/// Draws one frame into a texture and reads it back.
///
/// The camera is [`frame::default_camera`]: the same view the window starts
/// with, so the shot shows exactly what the window would.
pub fn take(gpu: &Gpu, width: u32, height: u32) -> Result<Shot, String> {
    take_scene(
        gpu,
        width,
        height,
        &frame::default_scene(frame::default_camera()),
    )
}

/// The same, but for a scene assembled by someone else.
///
/// This is the game's path to a PNG (ROADMAP J1), and it exists for the same
/// reason the shot itself does: "the window opened" is not a check that the
/// game drew anything. Same frame, same [`Frame`]; the only difference is who
/// built the scene.
pub fn take_scene(gpu: &Gpu, width: u32, height: u32, scene: &Scene) -> Result<Shot, String> {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot"),
        size: wgpu::Extent3d {
            width,
            height,
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

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("shot"),
        });

    // WARNING: the frame lives **to the end of the function**, not to the end
    // of the expression. Since T5c3 it owns an intermediate texture, and a
    // temporary `Frame::new(...).draw(...)` released it along with itself
    // before the commands were submitted to the device. This looked not like a
    // bug but like "the tonemapper does not work": the frame was drawn
    // correctly, only the compression pass read a texture that was gone.
    let mut frame = Frame::new(gpu, FORMAT);
    frame.draw(gpu, &mut encoder, &view, width, height, scene);

    read_back(gpu, encoder, &texture, width, height)
}

/// Appends a texture-to-buffer copy to `encoder`, submits and reads the
/// result.
///
/// Separate from [`take`], because the frame is sometimes drawn elsewhere --
/// for instance in [`crate::depth_probe`], where a depth buffer joins the
/// colour.
pub fn read_back(
    gpu: &Gpu,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Shot, String> {
    // A buffer row must be a multiple of 256 bytes. Rather than demand a
    // "convenient" frame size, we pad and strip the padding while reading:
    // otherwise a 1920x1080 shot is simply impossible.
    let unpadded = width * 4;
    let padded = unpadded.div_ceil(256) * 256;

    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot readback"),
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
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

    let slice = buffer.slice(..);
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

    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        pixels.extend_from_slice(&data[start..start + unpadded as usize]);
    }

    drop(data);
    buffer.unmap();

    Ok(Shot {
        width,
        height,
        pixels,
    })
}
