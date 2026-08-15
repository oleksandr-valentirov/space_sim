//! Пристрій wgpu. Один на процес, спільний для вікна і для знімків.

/// Адаптер, пристрій і черга. Без вікна: surface приходить окремо, бо
/// знімки його не мають.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Чи вміє цей пристрій bindless-масиви текстур (ROADMAP-PLANETS.md, R5c).
    ///
    /// Полем, а не питанням до адаптера щоразу: рушій мусить знати відповідь
    /// **один раз** і однаково в усіх місцях. Без цього рельєф не малюється
    /// взагалі — правило 6 етапу R не дозволяє «спочатку класично».
    ///
    /// Три цілі проєкту (Vulkan, D3D12, Metal) це вміють — саме тому GL і
    /// відпав (PROJECT.md §7). Отже `false` тут означає бекенд, який і так не
    /// ціль, і мовчати про це не можна: той, хто просить рельєф, дістає
    /// помилку з назвою адаптера, а не порожній кадр.
    pub bindless: bool,
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

        // Bindless просимо, лише якщо адаптер його має: інакше `request_device`
        // впаде, і перша картинка не з'явилася б навіть там, де рельєф ніхто
        // не просив.
        let wanted = wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY;
        let bindless = adapter.features().contains(wanted);

        // downlevel_defaults, а не defaults: на етапі F нам нічого не
        // бракує, а нижча планка означає, що перша картинка запуститься
        // й там, де слабший бекенд. Піднімати — коли впремося.
        let mut limits = wgpu::Limits::downlevel_defaults();
        // Відбір у compute (R6b) читає п'ять storage-буферів: кандидати,
        // конуси, початки, вижилі й аргументи indirect. Downlevel дає чотири,
        // тож планка піднімається — це і є те «коли впремося», про яке
        // говорить коментар нижче. Просимо мінімум із того, що є в адаптера, і
        // того, що треба: більше не потрібно, менше не працює.
        limits.max_storage_buffers_per_shader_stage = limits
            .max_storage_buffers_per_shader_stage
            .max(6)
            .min(adapter.limits().max_storage_buffers_per_shader_stage);
        if bindless {
            // Впираємось саме сюди, і число не з голови: тайлсет Місяця з
            // п'ятьма рівнями піраміди — 2046 тайлів (R5b).
            limits.max_binding_array_elements_per_shader_stage = wgpu::Limits::default()
                .max_binding_array_elements_per_shader_stage
                .max(4096);
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("engine"),
            required_features: if bindless {
                wanted
            } else {
                wgpu::Features::empty()
            },
            required_limits: limits,
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
            bindless,
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
            "{:?} — {} ({:?}){}",
            info.backend,
            info.name,
            info.device_type,
            if self.bindless {
                ""
            } else {
                ", без bindless"
            }
        )
    }
}
