//! How many tiles a bindless array holds, and what they cost (stage T, T2).
//!
//! Step T2 has to name the level of the colour pyramid with a **number**, and
//! three numbers are named up front for that: `6*(4^0 + ... + 4^(L-1))` tiles,
//! bytes per tile, and how many textures the array holds. The first two are
//! arithmetic; the third is a property of the device, and it is the one that
//! must not be assumed.
//!
//! cargo run --release -p engine -- --tile-probe
//!
//! ## Why the declared limit is not enough
//!
//! `max_binding_array_elements_per_shader_stage` as measured by `gpu-probe` on
//! this machine is **1 048 576** (NVIDIA), **8 388 606** (RADV), **1 000 000**
//! (llvmpipe). The smallest of the three is **thirty times** larger than the
//! deepest pyramid we would ever consider. If the question hinged on it, it
//! would have been closed back on stage E.
//!
//! What it actually hits is something else, and something the adapter does not
//! report about itself:
//!
//! - **allocation granularity.** A tile is a few dozen texels on a side, i.e.
//!   single-digit kilobytes. The driver hands out memory in blocks, and how
//!   much of a block goes to waste is visible only from
//!   `generate_allocator_report()`: it carries both the sum of allocations and
//!   the sum of what is **reserved**. The difference between them is precisely
//!   the price of being small, and it cannot be computed from the file format;
//! - **time.** Thirty thousand `create_texture` calls are thirty thousand
//!   driver objects, and that time is paid when a body is loaded, not once per
//!   process lifetime.
//!
//! ## Why the probe makes its own device, and does so on every adapter
//!
//! `Gpu::new` asks for exactly `max(default, 4096)` elements -- a number
//! chosen for 2046 terrain tiles (`gpu.rs`). A probe measuring the limit
//! through it would be measuring that constant rather than the device. So here
//! there is a device of its own, with the limit equal to the adapter's.
//!
//! Adapters are taken **all hardware ones**, not one fastest, and precisely
//! because the question is about memory: a discrete card has its own VRAM, an
//! integrated one shares the system memory the game already lives in. A single
//! row from the discrete card would answer half the question. Software
//! adapters are skipped: in llvmpipe "GPU memory" is malloc, i.e. a number
//! that constrains nothing.
//!
//! ## What was measured (2026-08-16)
//!
//! **A tile costs x3.34 of what it carries, and that number is the same on
//! both vendors, in all three formats and at all three pyramid depths.** It
//! depends neither on the tile count nor on how many channels a tile has: 1225
//! bytes of data in `R8Unorm` turn into 4096 in memory, 2450 in `R16Sint` into
//! 8192. That is, **35x35 is not "a small texture", it is a four-kilobyte
//! one**, and it is that constant, not the declared array ceiling, that says
//! what a tile costs.
//!
//! Until X5b that also decided how many pyramid levels we could afford, since
//! the pyramid was resident whole. It no longer does: the GPU holds a pool of
//! slots and the pyramid stays in the file, so this constant now prices the
//! **pool** -- a fixed cost -- and pyramid depth is bounded by the source and
//! the disk instead.
//!
//! The only divergence between vendors is `Rgba8Unorm`: NVIDIA takes 12 288
//! bytes per tile, RADV 16 384. That is one more proof that a three-byte
//! colour saves nothing: what weighs 3 bytes per node in the file costs 12-16
//! kilobytes per tile in memory, against 4 for a single-channel one.
//!
//! The reserve grows in steps and equals zero on small pyramids (new textures
//! fit into a block already taken), while at level 7 it doubles the bill:
//! 128 MiB of allocations -> **256 MiB reserved**. Creation time is linear in
//! the tile count: ~11 ms for 2046, ~38 ms for 8190, ~165 ms for 32 766.
//!
//! ## What was measured for W1: the price of a baked slope (2026-08-16)
//!
//! **The halo costs nothing.** 33x33 and 35x35 give the same bytes per tile --
//! in every format, at every depth, on both vendors. So the 12.5% the halo
//! weighs in the file simply does not exist in GPU memory: both grids land in
//! one granularity block. Dropping the halo from the format is worth doing for
//! simplicity and disk, but **not for memory** -- there is nothing to buy
//! there.
//!
//! **The second channel costs exactly one granularity step, and the vendors
//! differ:**
//!
//! | format | NVIDIA | RADV |
//! |---|---|---|
//! | `R16Sint` (heights before stage W) | 8192 | 8192 |
//! | `Rg16Sint` (heights + slope) | **12 288** | **16 384** |
//!
//! That is x1.5 on the discrete card and x2 on the integrated one, and it is
//! the integrated one that pays out of the same memory the game lives in. In
//! scene numbers: Moon 16 -> 24/32 MiB, Earth 64 -> 96/128 MiB, together
//! **+40 MiB (NVIDIA)** and **+80 MiB (RADV)**. That is the price of the
//! answer to Q3, named before the format changed.
//!
//! WARNING: **Array binding time does not depend on the format at all** --
//! 1.0-1.1 ms for 8190 textures across all seven rows. That was the mechanism
//! behind debt D19: the driver paid for the **number** of textures and not for
//! their size, so a baked slope did not make it worse by a microsecond -- and
//! so the cure had to be binding fewer of them, which is what Y1 did.

use std::time::Instant;

use crate::tiles;

/// The levels worth measuring: 5 is the Moon's terrain, 6 its colour and both
/// of the Earth's pyramids, 7 the candidate T2 turned down.
///
/// 8 is the depth stage X5 aims at -- the one that reaches the source
/// (2.45 km per node against Blue Marble's 1.85 km). It is measured here
/// precisely because the frame will never allocate it: X5's whole claim is
/// that a resident pool costs the screen rather than the pyramid, and the
/// number that claim is compared against has to be a measurement rather than
/// an extrapolation of the granularity from 7.
const LEVELS: [u32; 4] = [5, 6, 7, 8];

/// The formats and grids T2 and W1 choose between.
///
/// `R16Sint` is what the heights carried before stage W, and it is here for
/// the sake of a scale to compare against. `R8Unorm` is one channel: the
/// global LROC WAC mosaic is monochrome, so for the Moon this is not a
/// simplification but exactly what the source holds. `Rgba8Unorm` is four,
/// because a three-byte texture format **does not exist at all** in wgpu:
/// `Rgb8` exists neither in WebGPU nor in Vulkan as a format sampleable
/// without extensions. So the "35^2*3" from the roadmap is a size in the file,
/// not in GPU memory.
///
/// `Rg16Sint` 33^2 is what the heights carry **now** (stage W): the baked
/// slope puts a second `i16` into the same texel. The rows with a halo stayed
/// in the table not out of nostalgia but because they are what gave the W1
/// answer: the grid here is a dimension of the table rather than a constant,
/// and without it "the halo costs 12.5%" would have stayed arithmetic over the
/// file instead of a measurement over memory.
const FORMATS: [(&str, wgpu::TextureFormat, usize, usize); 7] = [
    (
        "R16Sint 35^2 (heights before W)",
        wgpu::TextureFormat::R16Sint,
        2,
        tiles::STORED,
    ),
    (
        "R16Sint 33^2 (same, no halo)",
        wgpu::TextureFormat::R16Sint,
        2,
        tiles::NODES,
    ),
    (
        "Rg16Sint 35^2 (slope, w/ halo)",
        wgpu::TextureFormat::Rg16Sint,
        4,
        tiles::STORED,
    ),
    (
        "Rg16Sint 33^2 (heights today)",
        wgpu::TextureFormat::Rg16Sint,
        4,
        tiles::NODES,
    ),
    (
        "R8Unorm 35^2 (Moon colour, old)",
        wgpu::TextureFormat::R8Unorm,
        1,
        tiles::STORED,
    ),
    (
        "Rgba8Unorm 35^2 (Earth col, old)",
        wgpu::TextureFormat::Rgba8Unorm,
        4,
        tiles::STORED,
    ),
    (
        "Rgba8Unorm 33^2 (Earth col, now)",
        wgpu::TextureFormat::Rgba8Unorm,
        4,
        tiles::NODES,
    ),
];

struct Row {
    format: &'static str,
    levels: u32,
    tiles: usize,
    data_mib: f64,
    allocated_mib: f64,
    reserved_mib: f64,
    create_ms: f64,
    bind_ms: f64,
    failed: Option<String>,
}

/// Compute and print the table for every hardware adapter.
pub fn report() -> Result<(), String> {
    let instance = wgpu::Instance::default();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));

    let mut measured = 0;
    for adapter in &adapters {
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            continue;
        }
        match one_adapter(adapter) {
            Ok(()) => measured += 1,
            Err(e) => println!("{}: skipped -- {e}\n", adapter.get_info().name),
        }
    }

    if measured == 0 {
        return Err("no hardware adapter was measured".to_string());
    }
    Ok(())
}

/// The table for one adapter.
fn one_adapter(adapter: &wgpu::Adapter) -> Result<(), String> {
    let info = adapter.get_info();
    let ceiling = adapter.limits().max_binding_array_elements_per_shader_stage;
    println!(
        "adapter: {:?} -- {} ({:?})",
        info.backend, info.name, info.device_type
    );
    println!("declared array ceiling: {ceiling} elements");
    if ceiling == 0 {
        return Err("the adapter has no binding arrays -- nothing to measure".to_string());
    }

    let wanted = wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
        | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY;
    if !adapter.features().contains(wanted) {
        return Err("the full bindless set is missing".to_string());
    }

    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_binding_array_elements_per_shader_stage = ceiling;
    limits.max_binding_array_sampler_elements_per_shader_stage = adapter
        .limits()
        .max_binding_array_sampler_elements_per_shader_stage;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tile probe"),
        required_features: wanted,
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .map_err(|e| format!("the device does not come up: {e}"))?;

    let mut rows = Vec::new();
    for (name, format, bytes_per_texel, side) in FORMATS {
        for levels in LEVELS {
            rows.push(measure(
                &device,
                &queue,
                name,
                format,
                bytes_per_texel,
                side,
                levels,
            ));
        }
    }

    print_table(&rows);
    print_verdict(&rows);
    println!();
    Ok(())
}

/// One row of the table: build a pyramid of tiles, assemble an array out of
/// them, measure.
///
/// The textures live until the end of the function and die together with it --
/// the next row has to start from a clean slate, otherwise the allocator
/// report would show the sum of all the preceding measurements.
fn measure(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format_name: &'static str,
    format: wgpu::TextureFormat,
    bytes_per_texel: usize,
    nodes: usize,
    levels: u32,
) -> Row {
    let tiles_count = tiles::Terrain::count(levels);
    let side = nodes as u32;
    let data_bytes = tiles_count * nodes * nodes * bytes_per_texel;
    let pixels = vec![0u8; nodes * nodes * bytes_per_texel];

    let before = memory(device);
    let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);

    let start = Instant::now();
    let mut textures = Vec::with_capacity(tiles_count);
    let mut views = Vec::with_capacity(tiles_count);
    for _ in 0..tiles_count {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * bytes_per_texel as u32),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
        views.push(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        textures.push(texture);
    }
    // The queue flushes lazily, so without this the probe's time and memory
    // would turn out to be the time and memory of the queue write itself.
    queue.submit(std::iter::empty());
    let _ = device.poll(wait());
    let create_ms = start.elapsed().as_secs_f64() * 1e3;

    let start = Instant::now();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: sample_type(format),
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: std::num::NonZeroU32::new(tiles_count as u32),
        }],
    });
    let borrowed: Vec<&wgpu::TextureView> = views.iter().collect();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureViewArray(&borrowed),
        }],
    });
    let _ = device.poll(wait());
    let bind_ms = start.elapsed().as_secs_f64() * 1e3;

    let after = memory(device);
    let allocated = after.0.saturating_sub(before.0);
    let reserved = after.1.saturating_sub(before.1);
    let failed = pollster::block_on(scope.pop()).map(|e| e.to_string());

    drop(group);
    drop(views);
    drop(textures);
    let _ = device.poll(wait());

    Row {
        format: format_name,
        levels,
        tiles: tiles_count,
        data_mib: data_bytes as f64 / (1024.0 * 1024.0),
        allocated_mib: allocated as f64 / (1024.0 * 1024.0),
        reserved_mib: reserved as f64 / (1024.0 * 1024.0),
        create_ms,
        bind_ms,
        failed,
    }
}

/// `poll(Wait)` with no limits -- the same call as in `shot.rs`.
fn wait() -> wgpu::PollType {
    wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    }
}

fn sample_type(format: wgpu::TextureFormat) -> wgpu::TextureSampleType {
    match format {
        wgpu::TextureFormat::R16Sint | wgpu::TextureFormat::Rg16Sint => {
            wgpu::TextureSampleType::Sint
        }
        _ => wgpu::TextureSampleType::Float { filterable: true },
    }
}

/// Two memory numbers: the sum of allocations and the sum of what is reserved,
/// bytes.
///
/// Both, and that is not redundancy -- they answer different questions, and
/// **the probe did not count the first one at first and so saw zeroes**. The
/// reserve grows in **blocks**: as long as new textures fit into a block
/// already taken, the difference between "before" and "after" is zero, and the
/// row looks free even though memory was spent. The sum of allocations has no
/// such hole -- it counts every texture separately, alignment included, i.e.
/// exactly the granularity the probe was written for.
///
/// The reserve stays as the second column because it says something else: how
/// much the device **holds** on our behalf, including the unused tails of
/// blocks. A backend without a report (GL) gives zeroes, and in the table that
/// shows up as zeroes rather than as "costs nothing".
fn memory(device: &wgpu::Device) -> (u64, u64) {
    device
        .generate_allocator_report()
        .map(|report| (report.total_allocated_bytes, report.total_reserved_bytes))
        .unwrap_or((0, 0))
}

fn print_table(rows: &[Row]) {
    println!();
    println!(
        "format                           levels   tiles   data, MiB  alloc, MiB  reserv, MiB  make, ms  array, ms"
    );
    for row in rows {
        println!(
            "{:32} {:6}  {:6}   {:9.2}  {:10.2}  {:11.2}  {:8.1}  {:9.1}{}",
            row.format,
            row.levels,
            row.tiles,
            row.data_mib,
            row.allocated_mib,
            row.reserved_mib,
            row.create_ms,
            row.bind_ms,
            match &row.failed {
                Some(e) => format!("  <- {e}"),
                None => String::new(),
            }
        );
    }
}

/// A verdict, not just data -- the same rule as in `gpu-probe`.
fn print_verdict(rows: &[Row]) {
    println!();
    println!("Verdict\n");

    for row in rows {
        if row.failed.is_some() {
            println!(
                "  {} at {} levels ({} tiles) DID NOT FIT",
                row.format, row.levels, row.tiles
            );
        }
    }

    // The granularity overhead is the probe's headline number: it says how
    // much memory the smallness of a tile eats by itself, and it is that, not
    // the declared ceiling, that limits the depth of the pyramid.
    for row in rows {
        if row.data_mib > 0.0 && row.allocated_mib > 0.0 {
            println!(
                "  {} / {} levels: allocation x{:.2} of the data, {:.0} bytes per tile",
                row.format,
                row.levels,
                row.allocated_mib / row.data_mib,
                row.allocated_mib * 1024.0 * 1024.0 / row.tiles as f64
            );
        }
    }
}
