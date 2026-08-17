//! The aerial perspective volume bands the surface from low orbit (debt D17).
//!
//! ## Why this file exists before any fix
//!
//! D17 was found by eye during stage V and stayed "someone should look from
//! this angle" for two stages. It now has a number -- and a number that lives
//! only in a throwaway script is a memory, not a regression. Stage `Y` will add
//! layers on top of this air, and "it looks better now" is not something a layer
//! can be held to.
//!
//! ## The metric, and what makes it a metric
//!
//! A row of the frame minus its own moving average, then the range of what is
//! left. The moving average removes the smooth part -- the sphere's curvature,
//! the sun's gradient, the limb -- and leaves only what changes faster than the
//! window. Banding changes exactly that fast.
//!
//! WARNING: the number means nothing on its own, and this is the whole point of
//! the pairing below. **Every frame is measured twice: with air and without,
//! from the same camera.** Without the second half the metric would also be
//! reading relief, mosaic and tonemapper, and would call all three banding. The
//! body here is smooth and unpainted for the same reason: what is left after the
//! subtraction is then the air and nothing else.
//!
//! ## Two angles, because the diagnosis is about angles
//!
//! The volume's slices are laid out over a span that `atmosphere::aerial_span`
//! computes **per frame** -- from the camera to the tangent through the air --
//! while each ray stops at whatever it hits. Looking down from 400 km the ray
//! stops at the ground after 400 km of a 3426 km span, so only the first six of
//! thirty-two slices do any work; along the limb the ray runs the whole span and
//! all thirty-two do. So the model predicts banding at nadir and none at the
//! limb, and the test checks both. A "fix" that blurred the whole frame would
//! erase that asymmetry, and this file would notice.

use engine::camera::Camera;
use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::scene::{Atmosphere, Body, Scene, TileSet};
use engine::shot::{self, Shot};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

/// Earth's radius, metres -- a smooth sphere, no asset needed.
const RADIUS: f64 = 6_371_000.0;

/// Station altitude, metres. The altitude is not decoration: `aerial_span`
/// gives `near = r - top = 300 km` and `far = 3426 km` here, and it is that
/// ratio which decides how many slices a downward ray gets.
const ALTITUDE: f64 = 400_000.0;

/// The moving-average window as a fraction of the frame width.
///
/// A fraction rather than a count of pixels: the ring spacing on screen scales
/// with the resolution, so a fixed 81 px would mean something different at every
/// size. One twelfth is the 81 px of the original measurement at 960 px wide.
const WINDOW: u32 = WIDTH / 12;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// The frame from `altitude`, looking `pitch` degrees away from nadir.
///
/// WARNING: the frame's up vector cannot be the local vertical. At nadir it is
/// parallel to the view direction, the camera basis degenerates and the frame
/// comes out solid black with no diagnostic at all. This pair is orthogonal to
/// the view at every pitch: north at nadir, the local vertical at the limb.
fn look(gpu: &Gpu, frame: &mut Frame, pitch: f64, air: bool) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aerial banding"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // The local basis under the station: up, north, and the view direction
    // tilted from nadir towards north.
    let up = [1.0, 0.0, 0.0];
    let north = [0.0, 0.0, 1.0];
    let p = pitch.to_radians();
    let forward = [
        -p.cos() * up[0] + p.sin() * north[0],
        -p.cos() * up[1] + p.sin() * north[1],
        -p.cos() * up[2] + p.sin() * north[2],
    ];
    let frame_up = [
        p.cos() * north[0] + p.sin() * up[0],
        p.cos() * north[1] + p.sin() * up[1],
        p.cos() * north[2] + p.sin() * up[2],
    ];

    let eye = up.map(|v| v * (RADIUS + ALTITUDE));
    let target = [
        eye[0] + forward[0] * 4.0e6,
        eye[1] + forward[1] * 4.0e6,
        eye[2] + forward[2] * 4.0e6,
    ];
    let mut scene = Scene::new(Camera::look_at(eye, target, frame_up));
    // The sun high over the station: the terminator is a step, and a step in
    // frame would be read as a fast change, i.e. as banding.
    scene.sun = [0.94, 0.0, 0.34];
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: RADIUS,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        // A dark surface, and that is the fixture's whole sensitivity. The
        // frame's default blue is an albedo of about 0.9 in that channel, i.e.
        // ten times a real ocean; over a surface that bright the air is a
        // rounding error and the metric reads one byte of quantisation. These
        // are Earth's own measured numbers (`Colour::mean` over `earth.col`).
        colour: [0.0595, 0.0595, 0.0732, 1.0],
        air: air.then(|| Atmosphere::EARTH.with_surface(RADIUS)),
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("aerial banding"),
        });
    frame.draw(gpu, &mut encoder, &view, WIDTH, HEIGHT, &scene);
    shot::read_back(gpu, encoder, &texture, WIDTH, HEIGHT).expect("the frame should have drawn")
}

/// The range of one row's brightness after its own moving average is removed.
///
/// The blue channel: the air is blue, so that is where it has the most to say.
/// The row is taken across the middle of the frame, where the surface fills it.
fn residual(shot: &Shot) -> f64 {
    let y = shot.height / 2;
    let row: Vec<f64> = (0..shot.width)
        .map(|x| f64::from(shot.pixel(x, y)[2]))
        .collect();

    let half = (WINDOW / 2) as usize;
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    // Only the interior: at the edges the window would be one-sided, i.e. the
    // trend would be biased and the bias would be counted as a residual.
    for x in half..row.len() - half {
        let mean: f64 = row[x - half..=x + half].iter().sum::<f64>() / (2 * half + 1) as f64;
        let value = row[x] - mean;
        low = low.min(value);
        high = high.max(value);
    }
    high - low
}

/// Air bands the frame looking down, and does not along the limb.
///
/// Both halves matter. The first is the defect: the measurement that named D17
/// is reproduced here as a number the tree keeps. The second is the diagnosis:
/// if the limb banded too, the cause would not be the slice distribution, and
/// every cure aimed at it would be aimed wrong.
///
/// The ceiling **is now the airless residual** (D17d). Until the fix landed it
/// sat above the defect and this test only said "the banding does not get
/// worse". The depth axis moved from distance to optical depth, the nadir
/// residual went 27.0 -> 14.3 -> **1.3 bytes**, and the ceiling came down with
/// it: from here on, air that bands the frame at all is a regression.
#[test]
fn the_air_bands_the_nadir_and_not_the_limb() {
    let Some(gpu) = gpu() else { return };
    let mut frame = Frame::new(&gpu, shot::FORMAT);

    // The frames land on disk: when this goes red one day, there will be
    // something to look at -- and while choosing a cure, something to measure
    // the ring positions on.
    let out = std::path::Path::new("build/d17");
    let mut report = Vec::new();
    for (name, pitch) in [("nadir", 0.0f64), ("limb", 88.0)] {
        let air = look(&gpu, &mut frame, pitch, true);
        let bare = look(&gpu, &mut frame, pitch, false);
        let (with, without) = (residual(&air), residual(&bare));
        println!("  {name}: with air {with:.1}, without {without:.1} bytes");
        let _ = air.write_png(&out.join(format!("{name}_air.png")));
        let _ = bare.write_png(&out.join(format!("{name}_bare.png")));
        report.push((name, with, without));
    }

    for (name, _, without) in &report {
        // The airless frame of a smooth sphere is smooth by construction: this
        // is what makes the metric a metric rather than a reading of the frame.
        // It must hold before and after any fix.
        assert!(
            *without <= 3.0,
            "{name}: the airless frame has a residual of {without:.1} bytes -- \
             the metric is reading something other than the air"
        );
    }

    let nadir = report[0].1;
    let limb = report[1].1;
    // The history this number carries: 27.0 bytes when D17 was diagnosed (32
    // slices), 14.3 after doubling them, 1.3 once the depth axis became optical
    // depth. The ceiling is the airless bound, because the residual is now at
    // the quantisation floor and there is nothing left to leave room for.
    //
    // The Earth screenshot that named D17 read 5.9 on the same metric, and the
    // difference is the fixture, not a disagreement: there the mosaic and the
    // relief carry most of the row, and the staircase rides on top of them. Here
    // the surface is uniform and dark, so nothing masks it. That is the point of
    // a fixture -- it is built to be sensitive, and 27 discriminated between
    // candidate cures where 5.9 would not.
    assert!(
        nadir <= 3.0,
        "looking down the air bands the frame by {nadir:.1} bytes -- D17 was \
         closed at 1.3, so this is a regression in the depth axis"
    );
    assert!(
        limb <= 4.0,
        "the limb banded too ({limb:.1} bytes): the cause is not the slice \
         distribution, and the cures aimed at it are aimed wrong"
    );
}
