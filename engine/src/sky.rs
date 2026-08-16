//! Небо й повітря на GPU: таблиці Hillaire 2020 (ROADMAP-ATMOSPHERE.md).
//!
//! Тут живуть **таблиці**, а не картинка. Розділення не косметичне: таблиці
//! різняться тим, як часто їх треба рахувати, і саме це визначає, де вони
//! стоять у кадрі (правило 5 етапу S):
//!
//! | таблиця | від чого залежить | як часто |
//! |---|---|---|
//! | пропускання | лише параметри повітря | раз на набір параметрів |
//!
//! Решта рядків з'явиться разом зі своїми кроками; заводити їх наперед
//! CLAUDE.md прямо забороняє.
//!
//! ## Чому [`Sky::ensure`] подає роботу сам, а не в чужий encoder
//!
//! Бо це не робота кадру. Таблиця пропускання перераховується тоді, коли
//! змінилися параметри повітря, тобто практично ніколи; протягнута крізь
//! encoder кадру, вона виглядала б як щокадрова, і перший, хто прийде її
//! оптимізувати, витратить день. Таблиці, які **справді** рахуються щокадру,
//! підуть у кадровий encoder — і різниця між ними стане видима з коду.

use crate::atmosphere;
use crate::gpu::Gpu;
use crate::scene::Atmosphere;

/// WGSL, згенерований зі `shaders/sky.slang` (`scripts/build_shaders.sh`).
const SKY_WGSL: &str = include_str!("../shaders/sky.wgsl");

/// Формат таблиць. Half-float: пропускання лежить у `[0, 1]`, і одинадцяти
/// значущих бітів там вистачає з запасом — виміряно тестом S2, який звіряє
/// таблицю з оракулом у `f64`.
const LUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Скільки байтів займає [`AirParams`] у шейдері: чотири `float4`.
const AIR_BYTES: u64 = 64;

/// Розмір групи в `transmittance_main` — те саме, що в `[numthreads(8, 8, 1)]`.
const GROUP: u32 = 8;

pub struct Sky {
    transmittance_pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    air_buffer: wgpu::Buffer,

    transmittance: wgpu::Texture,
    transmittance_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,

    /// Параметри, під які таблиці вже пораховані: саме повітря і радіус
    /// поверхні тіла, якому воно належить.
    ///
    /// Радіус окремо, бо в [`Atmosphere`] його немає: там лише верхня межа.
    /// Два тіла з однаковим повітрям і різними радіусами — різні атмосфери, і
    /// ключ мусить це бачити.
    current: Option<(Atmosphere, f64)>,
}

impl Sky {
    pub fn new(gpu: &Gpu) -> Sky {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sky"),
                source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
            });

        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sky luts"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: std::num::NonZeroU64::new(AIR_BYTES),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: LUT_FORMAT,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
            });

        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });

        let transmittance_pipeline =
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("transmittance"),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some("transmittance_main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        let air_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("air params"),
            size: AIR_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // `COPY_SRC` тут заради перевірки, і це не приховується: оракул S2 —
        // звірка таблиці з `engine::atmosphere`, і прочитати її можна лише
        // звідси. Та сама причина, що в `indirect_buffer` (R6b).
        let transmittance = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transmittance lut"),
            size: wgpu::Extent3d {
                width: atmosphere::TRANSMITTANCE_WIDTH,
                height: atmosphere::TRANSMITTANCE_HEIGHT,
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
        });
        let transmittance_view = transmittance.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky luts"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
            ],
        });

        Sky {
            transmittance_pipeline,
            layout,
            air_buffer,
            transmittance,
            transmittance_view,
            bind_group,
            current: None,
        }
    }

    /// Таблиці під це повітря — порахувати, якщо вони ще не під нього.
    ///
    /// Повертає `true`, якщо рахувати таки довелося. Значення потрібне не
    /// кадру, а перевірці: «таблиця не перераховується щокадру» — твердження,
    /// яке треба вміти перевірити, а не лише написати в коментарі.
    pub fn ensure(&mut self, gpu: &Gpu, air: &Atmosphere, bottom_m: f64) -> bool {
        if self.current == Some((*air, bottom_m)) {
            return false;
        }

        gpu.queue
            .write_buffer(&self.air_buffer, 0, &air_bytes(air, bottom_m));

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
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(
                atmosphere::TRANSMITTANCE_WIDTH.div_ceil(GROUP),
                atmosphere::TRANSMITTANCE_HEIGHT.div_ceil(GROUP),
                1,
            );
        }
        gpu.queue.submit([encoder.finish()]);

        self.current = Some((*air, bottom_m));
        true
    }

    /// Вигляд таблиці пропускання — для тих, хто її читатиме в шейдері.
    pub fn transmittance_view(&self) -> &wgpu::TextureView {
        &self.transmittance_view
    }

    /// Макет групи прив'язки таблиць. Публічний рівно тому, що на нього
    /// спиратимуться наступні кроки етапу.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Таблиця пропускання назад у пам'ять — оракул S2.
    ///
    /// Читати це в кадрі не можна: тут `poll(Wait)`, тобто повна зупинка
    /// конвеєра. Існує рівно заради перевірки, як і [`crate::frame::Frame::drawn_patches`].
    ///
    /// Рядок-major, `[r, g, b, a]` на тексель, уже розпакований із half-float.
    pub fn read_transmittance(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        let width = atmosphere::TRANSMITTANCE_WIDTH;
        let height = atmosphere::TRANSMITTANCE_HEIGHT;
        // Вісім байтів на тексель; 256 текселів у рядку дають 2048 — кратне
        // 256, тобто вирівнювання `copy_texture_to_buffer` виконується саме
        // собою й окремого доповнення не треба.
        let bytes_per_row = width * 8;
        assert_eq!(bytes_per_row % 256, 0, "рядок таблиці не вирівняний");

        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("transmittance readback"),
            size: u64::from(bytes_per_row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("transmittance readback"),
            });
        encoder.copy_texture_to_buffer(
            self.transmittance.as_image_copy(),
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
            .map_err(|e| format!("не дочекалися GPU: {e}"))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| format!("буфер не відобразився: {e}"))?;

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
}

/// Параметри повітря в розкладці `AirParams` із `sky.slang`.
///
/// Виписано руками з тієї самої причини, що й `Uniforms::to_bytes` у кадрі:
/// наш `unsafe` живе лише в `core-rs` (CLAUDE.md, інваріант 1).
///
/// **Радіуси звужуються до `f32` тут, і це не те звуження, якого бояться.**
/// Правило «світові координати ніколи не в float» (F4) стосується позицій, у
/// яких камера віднімається від великого числа; радіус тіла в неї не входить
/// — з нього рахують висоту над поверхнею, а `6.371·10⁶` у `f32` має крок
/// 0.5 м, тобто помилку в шістнадцять мільйонних від висоти шкали.
fn air_bytes(air: &Atmosphere, bottom_m: f64) -> Vec<u8> {
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
    bytes
}

/// Half-float у `f32`.
///
/// Десять рядків замість залежності: `half` уже є в дереві транзитивно, але
/// прямою залежністю рушія стала б заради однієї функції, потрібної лише
/// перевірці. Формат IEEE 754 binary16 описаний повністю — знак, п'ять бітів
/// порядку зі зсувом 15, десять бітів мантиси.
fn from_half(bits: u16) -> f32 {
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;
    let magnitude = match exponent {
        // Нуль і субнормальні: значення `mantissa · 2⁻²⁴`.
        0 => f32::from(mantissa) * (1.0 / 16_777_216.0),
        // Нескінченність і NaN — у таблиці їх бути не може, але мовчки
        // перетворити їх на скінченне число означало б сховати помилку.
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

    /// Розпакування half-float — на числах, які легко перевірити руками.
    #[test]
    fn half_floats_unpack_to_the_numbers_they_encode() {
        assert_eq!(from_half(0x0000), 0.0);
        assert_eq!(from_half(0x3c00), 1.0);
        assert_eq!(from_half(0x4000), 2.0);
        assert_eq!(from_half(0xc000), -2.0);
        assert_eq!(from_half(0x3800), 0.5);
        // Найменше нормальне: 2⁻¹⁴.
        assert_eq!(from_half(0x0400), 2.0f32.powi(-14));
        // Найбільше субнормальне: (1023/1024)·2⁻¹⁴.
        assert!((from_half(0x03ff) - 1023.0 / 1024.0 * 2.0f32.powi(-14)).abs() < 1.0e-12);
        assert!(from_half(0x7c00).is_infinite());
        assert!(from_half(0x7e00).is_nan());
    }

    /// Розкладка `AirParams` — та сама, що в шейдері: шістнадцять чисел,
    /// і кожне на своєму місці.
    #[test]
    fn the_air_params_land_where_the_shader_reads_them() {
        let air = Atmosphere::EARTH;
        let bytes = air_bytes(&air, 6_371_000.0);
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
    }
}
