//! The star background in a frame (ROADMAP, stage Z, Z3).
//!
//! Three claims, and each catches a different half of the pass.
//!
//! 1. **A star lands where its direction says.** This is the projection, and
//!    it is the part that fails silently: a swapped axis or a sign puts the
//!    whole sky somewhere else, and a sky in the wrong place still looks like
//!    a sky. The check therefore uses a direction that is off every axis of
//!    symmetry -- straight ahead would pass with the `right` and `up` vectors
//!    exchanged.
//! 2. **Five magnitudes are a hundredfold on screen, not just in the code.**
//!    The ratio is taken between two renders of the *same* star position, so
//!    the quad's geometry and the pixel it lands on are identical and cancel;
//!    what is left is the flux conversion, through the buffer, the shader and
//!    the tonemapper. Both magnitudes are chosen to stay under the
//!    tonemapper's knee, where the curve is the identity -- above it the ratio
//!    would be compressed, and the test would be measuring the tonemapper.
//! 3. **A star behind the camera is not drawn.** The vertex shader cannot
//!    discard, so it pushes such a star off screen by hand; if that arithmetic
//!    is wrong the star reappears mirrored in front, which is the sort of
//!    thing nobody notices until a constellation is doubled.

use engine::camera::Camera;
use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::scene::Scene;
use engine::shot::{self, Shot};
use engine::srgb;
use engine::stars::{Catalogue, Star};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// A sky of exactly the stars given, drawn from the origin looking down -x.
///
/// The camera looks at nothing: an empty scene, so the only thing in the frame
/// is the background. Deliberate -- with a planet in shot the brightest pixel
/// would be the planet.
fn shoot(gpu: &Gpu, stars: Vec<Star>) -> Shot {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    frame.load_stars(gpu, &Catalogue { stars });

    let scene = Scene::new(Camera::look_at(
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    ));

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("stars"),
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
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("stars"),
        });
    frame.draw(gpu, &mut encoder, &view, WIDTH, HEIGHT, &scene);
    shot::read_back(gpu, encoder, &texture, WIDTH, HEIGHT).expect("the shot should read back")
}

/// The brightest pixel, and how far it rises **above the background**.
///
/// Above the background rather than absolute, and the first version of this
/// file got it wrong in a way worth keeping a note about: the frame is not
/// cleared to black. `frame::CLEAR` is a very dark blue whose red channel is
/// 0.0015 linear, which is the same order as a sixth-magnitude star -- so the
/// absolute peak of an empty sky is not zero, and a faint star measured
/// absolutely comes out twice as bright as it is. Blending is additive, so
/// subtracting the clear colour is exact rather than approximate.
fn brightest(shot: &Shot) -> ((u32, u32), f64) {
    let background = engine::frame::CLEAR.r;
    let mut best = ((0, 0), f64::NEG_INFINITY);
    for y in 0..shot.height {
        for x in 0..shot.width {
            let value = srgb::to_linear(f64::from(shot.pixel(x, y)[0]) / 255.0) - background;
            if value > best.1 {
                best = ((x, y), value);
            }
        }
    }
    best
}

fn star(dir: [f32; 3], magnitude: f32) -> Star {
    let length = (dir.iter().map(|v| v * v).sum::<f32>()).sqrt();
    Star {
        dir: dir.map(|v| v / length),
        magnitude,
        colour_index: 0.0,
    }
}

/// A star lands where its direction says it should.
#[test]
fn a_star_lands_where_its_direction_points() {
    let Some(gpu) = gpu() else {
        eprintln!("SKIPPED: no adapter");
        return;
    };

    // The camera looks along +x with +z up, so `right` is -y. A direction
    // tilted towards -y and +z must therefore land right of centre and above
    // it -- and the two tilts differ, so a swap of the axes moves the star.
    let tilt_right = 0.20_f32;
    let tilt_up = 0.10_f32;
    let shot = shoot(&gpu, vec![star([1.0, -tilt_right, tilt_up], 0.0)]);

    let ((x, y), value) = brightest(&shot);
    assert!(value > 0.01, "no star in the frame at all: peak {value}");

    // Where the projection says it should be. `tan(fov/2)` vertically and that
    // times the aspect horizontally -- the same numbers the pass is given.
    let t = (engine::frame::FOV_Y / 2.0).tan();
    let aspect = f64::from(WIDTH) / f64::from(HEIGHT);
    let ndc_x = f64::from(tilt_right) / (t * aspect);
    let ndc_y = f64::from(tilt_up) / t;
    let want_x = (ndc_x * 0.5 + 0.5) * f64::from(WIDTH);
    // Screen y grows downwards while NDC y grows upwards.
    let want_y = (0.5 - ndc_y * 0.5) * f64::from(HEIGHT);

    assert!(
        (f64::from(x) - want_x).abs() <= 2.0 && (f64::from(y) - want_y).abs() <= 2.0,
        "the star is at ({x}, {y}) and the projection says ({want_x:.1}, {want_y:.1})"
    );
}

/// Five magnitudes are a factor of a hundred on screen too.
#[test]
fn five_magnitudes_are_a_hundredfold_in_the_frame() {
    let Some(gpu) = gpu() else {
        eprintln!("SKIPPED: no adapter");
        return;
    };

    // The same direction in both renders, so the quad falls on the same
    // pixels and its shape cancels out of the ratio.
    let dir = [1.0, -0.20, 0.10];
    // Magnitude 1 is 0.199 linear and magnitude 6 is 0.00199 -- both well
    // under the tonemapper's knee at 0.8, where the curve is the identity.
    // The faint one is the same order as the clear colour, which is exactly
    // why `brightest` subtracts it.
    let bright = brightest(&shoot(&gpu, vec![star(dir, 1.0)])).1;
    let faint = brightest(&shoot(&gpu, vec![star(dir, 6.0)])).1;

    assert!(bright > 0.05, "the bright star is missing: {bright}");
    assert!(faint > 0.0, "the faint star is missing entirely");

    let ratio = bright / faint;
    // The tolerance is eight bytes' worth at the faint end: the faint star
    // sits near the bottom of the sRGB scale, where one byte is a percent of
    // its own value, and no amount of correct arithmetic survives that
    // exactly.
    assert!(
        (ratio - 100.0).abs() < 12.0,
        "five magnitudes gave a factor of {ratio:.1} on screen ({bright:.5} against {faint:.5})"
    );
}

/// A star behind the camera stays behind it.
#[test]
fn a_star_behind_the_camera_is_not_drawn() {
    let Some(gpu) = gpu() else {
        eprintln!("SKIPPED: no adapter");
        return;
    };

    // Directly behind, and slightly off axis so that a mirrored star would
    // land somewhere findable rather than exactly at a corner.
    let shot = shoot(&gpu, vec![star([-1.0, -0.20, 0.10], -1.0)]);
    let (_, value) = brightest(&shot);

    assert!(
        value < 1.0e-4,
        "something was drawn for a star behind the camera: peak {value}"
    );
}
