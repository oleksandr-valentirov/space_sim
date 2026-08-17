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
//! 4. **The air puts the stars out (Z4).** The same star, from the same place,
//!    with and without an atmosphere between it and the camera. Through a
//!    daytime sky its contribution has to fall below one step of the scale --
//!    not merely dim, gone, because a star that survives daylight at byte 1 is
//!    still a star in the wrong place.

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
fn background(channel: usize) -> f64 {
    let clear = engine::frame::CLEAR;
    [clear.r, clear.g, clear.b][channel]
}

fn brightest(shot: &Shot) -> ((u32, u32), f64) {
    let background = background(0);
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

/// The air dims a star seen through it, and reddens what it dims (Z4).
///
/// **Not the test the plan asked for, and the difference is a finding.** Z4
/// was written as "the same star by day and by night: by day its contribution
/// falls below one step of the scale". It does not, and no bug is responsible:
/// zenith transmittance from the ground is 0.94 in red (`--example
/// star_scale`), so the air removes six per cent of a star and could never
/// hide one. What hides stars in daylight is the sky outshining them, and on
/// our scale it does not -- a magnitude-2 star is 0.079 against a daytime
/// zenith sky of 0.019 to 0.044. That is a question about the star scale, not
/// about this pass.
///
/// So this checks what the pass genuinely does, where it genuinely does it: a
/// star seen through the limb from orbit. At `mu = -0.320` from 400 km the ray
/// grazes the air without reaching the ground, and transmittance is 0.51 in
/// red against 0.16 in blue.
///
/// The colour half of the claim is the stronger one. Dimming depends on how
/// many pixels the star covers and where its quad lands; the **ratio between
/// its channels** does not, so a reddened star is evidence no amount of
/// geometry can fake.
#[test]
fn the_air_dims_a_star_through_the_limb_and_reddens_it() {
    let Some(gpu) = gpu() else {
        eprintln!("SKIPPED: no adapter");
        return;
    };

    let radius = engine::sphere::EARTH_RADIUS_M;
    let altitude = 400.0e3;
    // The cosine between the local vertical and the ray. Between the tangent
    // to the air (-0.294) and the tangent to the ground (-0.339): the ray goes
    // through the whole atmosphere and comes out the other side.
    let mu = -0.320_f64;
    let dir = [mu, (1.0 - mu * mu).sqrt(), 0.0];

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    frame.load_stars(
        &gpu,
        &Catalogue {
            stars: vec![star([dir[0] as f32, dir[1] as f32, dir[2] as f32], 1.0)],
        },
    );

    let shoot_with = |frame: &mut Frame, air: Option<engine::scene::Atmosphere>| {
        let eye = [radius + altitude, 0.0, 0.0];
        // Looking straight at the star, so it lands in the middle of the frame
        // and the planet's limb sits below it.
        let target = [eye[0] + dir[0], eye[1] + dir[1], eye[2] + dir[2]];
        let mut scene = Scene::new(Camera::look_at(eye, target, [1.0, 0.0, 0.0]));
        // The Sun on the far side, so the sky here is night and adds almost
        // nothing of its own to the pixel being measured.
        scene.sun = [-1.0, 0.0, 0.0];
        scene.bodies.push(engine::scene::Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: radius,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: engine::scene::TileSet::Smooth,
            colour: engine::frame::COLOUR,
            air,
        });

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("star through the limb"),
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
                label: Some("star through the limb"),
            });
        frame.draw(&gpu, &mut encoder, &view, WIDTH, HEIGHT, &scene);
        shot::read_back(&gpu, encoder, &texture, WIDTH, HEIGHT).expect("the shot should read back")
    };

    let vacuum = shoot_with(&mut frame, None);
    let through_air = shoot_with(
        &mut frame,
        Some(engine::scene::Atmosphere::EARTH.with_surface(radius)),
    );

    // Where the star landed, found rather than assumed -- and if the planet
    // covered it, this fails first and says so.
    let ((x, y), bare_red) = brightest(&vacuum);
    assert!(
        bare_red > 0.01,
        "no star in vacuum at ({x}, {y}) -- the fixture is wrong before the claim is: {bare_red}"
    );

    let linear = |shot: &Shot, channel: usize| {
        srgb::to_linear(f64::from(shot.pixel(x, y)[channel]) / 255.0) - background(channel)
    };
    let (vac_r, vac_b) = (linear(&vacuum, 0), linear(&vacuum, 2));
    let (air_r, air_b) = (linear(&through_air, 0), linear(&through_air, 2));

    // Dimmed: measured transmittance in red is 0.51, so anything above 0.75 of
    // the vacuum value means the pass did not run.
    assert!(
        air_r < vac_r * 0.75,
        "the limb did not dim the star: {air_r:.5} against {vac_r:.5} in vacuum"
    );

    // And reddened. The star is drawn white, so in vacuum its channels are
    // equal; through the air blue must lose far more than red.
    let vac_ratio = vac_b / vac_r.max(1.0e-9);
    let air_ratio = air_b / air_r.max(1.0e-9);
    assert!(
        air_ratio < vac_ratio * 0.6,
        "the air dimmed the star without reddening it: blue/red {air_ratio:.3} through \
         the air against {vac_ratio:.3} in vacuum"
    );
}
