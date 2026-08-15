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
        let ask = |fallback: bool| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: fallback,
                compatible_surface: compatible,
                ..Default::default()
            }))
        };

        // Немає апаратного — беремо програмний (ROADMAP-UI.md, U6c). На
        // Windows це WARP, тобто D3D12 на процесорі; на Linux ту саму роль
        // грає lavapipe, який CI ставить пакетом.
        //
        // Порядок саме такий, а не `force_fallback_adapter` завжди: на
        // машині з відеокартою програмний растеризатор був би в сотні разів
        // повільнішим і мовчки підмінив би те, що міряють зонди.
        let adapter = match ask(false) {
            Ok(adapter) => adapter,
            Err(hardware) => ask(true).map_err(|software| {
                format!("немає придатного адаптера: {hardware}; програмного теж: {software}")
            })?,
        };

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

    /// Пристрій для тестів, або `None`, якщо адаптера немає взагалі.
    ///
    /// **Мовчазний пропуск — це зелений тест, який нічого не перевіряє**, тож
    /// на машині, де GPU має бути, пропуск має бути помилкою. Це вмикає
    /// змінна середовища `SPACE_SIM_REQUIRE_GPU`: локально її немає (не в
    /// кожного під рукою драйвер), у CI вона стоїть скрізь, де адаптер
    /// зобов'язаний знайтися.
    ///
    /// Друкує назву адаптера завжди. Без цього рядка в логу CI неможливо
    /// відрізнити «тести пройшли на WARP» від «тести пройшли на чомусь
    /// іншому» — а це різні твердження (U6c).
    pub fn for_tests() -> Option<Gpu> {
        match Gpu::new(wgpu::Instance::default(), None) {
            Ok(gpu) => {
                eprintln!("адаптер: {}", gpu.describe());
                Some(gpu)
            }
            Err(e) => {
                assert!(
                    std::env::var_os("SPACE_SIM_REQUIRE_GPU").is_none(),
                    "SPACE_SIM_REQUIRE_GPU задано, а адаптера немає: {e}"
                );
                eprintln!("ПРОПУЩЕНО: немає адаптера wgpu ({e})");
                None
            }
        }
    }

    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!(
            "{:?} — {} ({:?})",
            info.backend, info.name, info.device_type
        )
    }
}
