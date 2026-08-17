//! The wgpu device. One per process, shared by the window and the
//! screenshots.

/// Adapter, device and queue. No window: the surface arrives separately,
/// because screenshots have none.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Whether this device can do bindless texture arrays
    /// (ROADMAP-PLANETS.md, R5c).
    ///
    /// A field rather than a question asked of the adapter each time: the
    /// engine must know the answer **once** and identically everywhere.
    /// Without it terrain is not drawn at all -- rule 6 of stage R does not
    /// allow "the classic way first".
    ///
    /// All three targets of the project (Vulkan, D3D12, Metal) can do it --
    /// which is why GL was dropped (PROJECT.md §7). So `false` here means a
    /// backend that is not a target anyway, and it must not be passed over in
    /// silence: whoever asks for terrain gets an error naming the adapter
    /// rather than an empty frame.
    pub bindless: bool,
}

impl Gpu {
    /// `compatible` is only needed for a window: the adapter must be able to
    /// draw into that particular surface. Screenshots pass `None`.
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

        // No hardware one -- take a software one (ROADMAP-UI.md, U6c). On
        // Windows that is WARP, i.e. D3D12 on the CPU; on Linux the same role
        // is played by lavapipe, which CI installs as a package.
        //
        // In this order rather than `force_fallback_adapter` always: on a
        // machine with a graphics card a software rasteriser would be hundreds
        // of times slower and would silently replace what the probes measure.
        let adapter = match ask(false) {
            Ok(adapter) => adapter,
            Err(hardware) => ask(true).map_err(|software| {
                format!("no suitable adapter: {hardware}; no software one either: {software}")
            })?,
        };

        // Bindless is requested only if the adapter has it: otherwise
        // `request_device` fails and the first picture would not appear even
        // where nobody asked for terrain.
        let wanted = wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY;
        let bindless = adapter.features().contains(wanted);

        // downlevel_defaults rather than defaults: nothing is missing for
        // stage F, and a lower bar means the first picture also runs on a
        // weaker backend. Raise it when we run into it.
        let mut limits = wgpu::Limits::downlevel_defaults();
        // Culling in compute (R6b) reads five storage buffers: candidates,
        // cones, origins, survivors and the indirect arguments. Downlevel
        // gives four, so the bar goes up -- this is the "when we run into it"
        // the comment above speaks of. We ask for the minimum of what the
        // adapter has and what is needed: more is unnecessary, less does not
        // work.
        limits.max_storage_buffers_per_shader_stage = limits
            .max_storage_buffers_per_shader_stage
            .max(6)
            .min(adapter.limits().max_storage_buffers_per_shader_stage);
        if bindless {
            // This is where we run into it, and the number is not invented:
            // the Moon's **colour** tileset with six pyramid levels is 8190
            // tiles (T2a). Heights over five levels are 2046, four times
            // fewer, which is why this number used to be 4096.
            limits.max_binding_array_elements_per_shader_stage = wgpu::Limits::default()
                .max_binding_array_elements_per_shader_stage
                .max(8192);
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
        .map_err(|e| format!("the device cannot be created: {e}"))?;

        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
            bindless,
        })
    }

    /// A device for tests, or `None` if there is no adapter at all.
    ///
    /// **A silent skip is a green test that checks nothing**, so on a machine
    /// where a GPU should exist a skip must be an error. The
    /// `SPACE_SIM_REQUIRE_GPU` environment variable switches that on: locally
    /// it is unset (not everyone has a driver to hand), in CI it is set
    /// everywhere an adapter is obliged to be found.
    ///
    /// It always prints the adapter name. Without that line in the CI log
    /// there is no telling "the tests passed on WARP" from "the tests passed
    /// on something else" -- and those are different claims (U6c).
    pub fn for_tests() -> Option<Gpu> {
        match Gpu::new(wgpu::Instance::default(), None) {
            Ok(gpu) => {
                eprintln!("adapter: {}", gpu.describe());
                Some(gpu)
            }
            Err(e) => {
                assert!(
                    std::env::var_os("SPACE_SIM_REQUIRE_GPU").is_none(),
                    "SPACE_SIM_REQUIRE_GPU is set, but there is no adapter: {e}"
                );
                eprintln!("SKIPPED: no wgpu adapter ({e})");
                None
            }
        }
    }

    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!(
            "{:?} -- {} ({:?}){}",
            info.backend,
            info.name,
            info.device_type,
            if self.bindless { "" } else { ", no bindless" }
        )
    }
}
