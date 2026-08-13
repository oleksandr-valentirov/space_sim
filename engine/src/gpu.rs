//! Пристрій wgpu. Один на процес, спільний для вікна і для знімків.

/// Адаптер, пристрій і черга. Без вікна: surface приходить окремо, бо
/// знімки його не мають.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// `compatible` потрібен лише для вікна: адаптер має вміти малювати саме
    /// в цю поверхню. Для знімків передається `None`.
    pub fn new(
        instance: wgpu::Instance,
        compatible: Option<&wgpu::Surface<'static>>,
    ) -> Result<Gpu, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: compatible,
            ..Default::default()
        }))
        .map_err(|e| format!("немає придатного адаптера: {e}"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("engine"),
            required_features: wgpu::Features::empty(),
            // downlevel_defaults, а не defaults: на етапі F нам нічого не
            // бракує, а нижча планка означає, що перша картинка запуститься
            // й там, де слабший бекенд. Піднімати — коли впремося.
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| format!("пристрій не створюється: {e}"))?;

        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }

    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!(
            "{:?} — {} ({:?})",
            info.backend, info.name, info.device_type
        )
    }
}
