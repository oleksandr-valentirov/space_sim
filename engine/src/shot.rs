//! Кадр у PNG, без вікна.
//!
//! Існує не заради зручності. «Вікно відкрилось і не впало» — не перевірка
//! рендера: чорний кадр виглядає точно так само, як правильний. Знімок можна
//! подивитися очима, звірити по пікселях і прогнати на CI, де вікна немає
//! взагалі.

use std::path::Path;

use crate::frame::{self, Frame};
use crate::gpu::Gpu;

/// Формат цілі. Не sRGB навмисно: у знімку хочемо ті самі байти, які
/// поклали, без перетворення на шляху, — інакше звірка кольору
/// перетворюється на звірку гамми.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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

/// Малює один кадр у текстуру й читає його назад.
///
/// Камера — [`frame::default_camera`]: той самий погляд, що й у вікні при
/// старті, тож знімок показує саме те, що показало б вікно.
pub fn take(gpu: &Gpu, width: u32, height: u32) -> Result<Shot, String> {
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

    Frame::new(gpu, FORMAT).draw(
        gpu,
        &mut encoder,
        &view,
        width,
        height,
        &frame::default_camera(),
    );

    read_back(gpu, encoder, &texture, width, height)
}

/// Дописує до `encoder` копіювання текстури в буфер, віддає команди й читає
/// результат.
///
/// Окремо від [`take`], бо кадр буває намальований деінде — наприклад у
/// [`crate::depth_probe`], де до кольору додається ще й буфер глибини.
pub fn read_back(
    gpu: &Gpu,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Shot, String> {
    // Рядок у буфері має бути кратним 256 байтам. Замість вимагати «зручний»
    // розмір кадру, дописуємо доповнення й зрізаємо його при читанні:
    // інакше знімок 1920×1080 просто не зробити.
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
        .map_err(|e| format!("не дочекалися GPU: {e}"))?;

    let data = slice
        .get_mapped_range()
        .map_err(|e| format!("буфер не відобразився: {e}"))?;

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
