//! The frame really does contain what it was meant to (ROADMAP F1, F2, I1).
//!
//! This is the same path that goes to the window: one `Frame` serves both. The
//! test catches not "it crashed" but "it drew the wrong thing" -- the case
//! where the window opens, the program does not fall over, and still nothing
//! works.
//!
//! Since I1 the frame draws a planet rather than a triangle, so the check got
//! stronger: the oracle is no longer "which channel dominates" but the share of
//! the frame the silhouette disc is obliged to cover -- `asin(R/(R+altitude))`,
//! the same formula F5 was measured with. A number against a number.

use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::scene::Scene;
use engine::shot::{self, Shot};
use engine::{flight_probe, sphere};

const SIZE: u32 = 128;

fn gpu() -> Option<Gpu> {
    // The engine's shared helper: it also decides whether skipping is allowed
    // (`SPACE_SIM_REQUIRE_GPU`, U6c) and prints the adapter name into the log.
    Gpu::for_tests()
}

fn coverage(shot: &Shot) -> f64 {
    let mut lit = 0u64;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] != frame::CLEAR_BYTES {
                lit += 1;
            }
        }
    }
    lit as f64 / (u64::from(shot.width) * u64::from(shot.height)) as f64
}

/// How much of the frame the planet should cover from altitude `altitude`.
fn expected_at(altitude: f64, width: u32, height: u32) -> f64 {
    let distance = sphere::EARTH_RADIUS_M + altitude;
    let half_angle = (sphere::EARTH_RADIUS_M / distance).asin();
    flight_probe::expected_coverage(half_angle, f64::from(width) / f64::from(height))
        .expect("the formula is defined only when the disc is wholly in the frame or the frame wholly in the disc")
}

/// The same from the default altitude.
fn expected(width: u32, height: u32) -> f64 {
    expected_at(frame::DEFAULT_ALTITUDE_M, width, height)
}

#[test]
fn the_background_stays_the_colour_we_asked_for() {
    let Some(gpu) = gpu() else { return };
    let taken = shot::take(&gpu, SIZE, SIZE).expect("the frame should have been drawn");

    assert_eq!(taken.width, SIZE);
    assert_eq!(
        taken.pixels.len(),
        (SIZE * SIZE * 4) as usize,
        "the row padding was not trimmed off"
    );

    // The corners are outside the planet's disc: from 1e7 m it covers about 42%
    // of the frame and does not reach the corners (F5).
    for (x, y) in [(1, 1), (SIZE - 2, 1), (1, SIZE - 2), (SIZE - 2, SIZE - 2)] {
        let pixel = taken.pixel(x, y);
        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            frame::CLEAR_BYTES,
            "pixel ({x}, {y}) should have stayed background"
        );
        assert_eq!(pixel[3], 255, "pixel ({x}, {y}) is not opaque");
    }
}

/// The planet covers exactly the share of the frame geometry demands.
///
/// This is the check of scale, camera and projection together: a sphere of
/// Earth's radius, camera-relative on every vertex, reversed-Z -- all on the
/// path the frame takes to the window, not in a separate probe.
#[test]
fn the_planet_covers_the_share_geometry_demands() {
    let Some(gpu) = gpu() else { return };
    let taken = shot::take(&gpu, SIZE, SIZE).expect("the frame should have been drawn");

    let measured = coverage(&taken);
    let analytic = expected(SIZE, SIZE);

    // One and a half percent of the frame is the discretisation limit at
    // 128x128 (the edge of the disc runs across pixels), not a margin just in
    // case: at 512x512 F5 got a discrepancy of 3e-4.
    assert!(
        (measured - analytic).abs() < 0.015,
        "coverage {measured:.4} against an analytic {analytic:.4}"
    );
}

/// The same `Frame` draws into a target of a different size.
///
/// That is why the depth texture lives inside `Frame`: if it does not follow
/// the target's size, wgpu validation falls over on mismatched attachments --
/// and if it does follow but the projection was not recomputed, the coverage
/// changes. So both frames are checked, each against its own aspect.
#[test]
fn one_frame_draws_into_two_different_sizes() {
    let Some(gpu) = gpu() else { return };

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let scene = frame::default_scene(frame::default_camera());

    // Wider, then smaller. There are no portrait ratios here deliberately: from
    // 1e7 m the disc is wider than the narrow side of such a frame, and the
    // analytic formula is undefined on a clipped disc
    // (`flight_probe::expected_coverage`).
    for (width, height) in [(SIZE, SIZE), (SIZE * 2, SIZE), (100, 100)] {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resize test"),
            size: wgpu::Extent3d {
                width,
                height,
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

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resize test"),
            });
        frame.draw(&gpu, &mut encoder, &view, width, height, &scene);

        let taken = shot::read_back(&gpu, encoder, &texture, width, height)
            .expect("the frame should have been read back");

        let measured = coverage(&taken);
        let analytic = expected(width, height);

        assert!(
            (measured - analytic).abs() < 0.015,
            "{width}x{height}: coverage {measured:.4} against an analytic {analytic:.4}"
        );
    }
}

/// The camera really does drive what gets drawn.
///
/// The `orbit` tests prove the arithmetic without a GPU; this one proves it
/// reaches the frame. The oracle is the same -- coverage against
/// `asin(R/(R+altitude))` -- so what is checked is not "the picture changed"
/// but "it changed exactly as it is obliged to".
#[test]
fn the_camera_moves_the_frame_it_draws() {
    let Some(gpu) = gpu() else { return };

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let mut orbit = Orbit::default();

    let far = draw(&gpu, &mut frame, &frame::default_scene(orbit.camera()));
    let far_coverage = coverage(&far);

    // Rotation must not change the coverage at all: a sphere is the same from
    // every side. This is the cheapest check that the rotation did not drag
    // altitude or projection along with it.
    orbit.drag(300.0, 120.0);
    let turned = draw(&gpu, &mut frame, &frame::default_scene(orbit.camera()));
    assert!(
        (coverage(&turned) - far_coverage).abs() < 0.005,
        "the rotation changed the coverage: {:.4} against {far_coverage:.4}",
        coverage(&turned)
    );

    // Zooming in, on the other hand, must -- and by exactly as much as geometry
    // says.
    for _ in 0..11 {
        orbit.zoom(1.0);
    }
    let near = draw(&gpu, &mut frame, &frame::default_scene(orbit.camera()));
    let measured = coverage(&near);
    let analytic = expected_at(orbit.altitude(), SIZE, SIZE);

    assert!(
        measured > far_coverage + 0.1,
        "zooming in changed nothing: {measured:.4} against {far_coverage:.4}"
    );
    assert!(
        (measured - analytic).abs() < 0.015,
        "from an altitude of {:.3e} m the coverage is {measured:.4} against an analytic {analytic:.4}",
        orbit.altitude()
    );
}

/// One `SIZE`x`SIZE` frame into a texture and back.
fn draw(gpu: &Gpu, frame: &mut Frame, scene: &Scene) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("camera test"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
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

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("camera test"),
        });
    frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, scene);

    shot::read_back(gpu, encoder, &texture, SIZE, SIZE)
        .expect("the frame should have been read back")
}
