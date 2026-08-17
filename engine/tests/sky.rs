//! The sky in the frame (ROADMAP-ATMOSPHERE.md, S4b).
//!
//! ## Why numbers here, not screenshots
//!
//! Screenshots are written too -- into `build/`, and they are worth looking at
//! -- but the eye does not decide. Rule 2 of stage S demands numbers, and the
//! numbers here are taken from the very physics the sky is supposed to show:
//!
//! - **near the horizon the sky is brighter and whiter than at the zenith.** A
//!   ray going shallowly passes through three times more air; the blue in it has
//!   time to scatter both back and sideways while the red gets through -- hence
//!   both the brightness and the whiteness;
//! - **a sunset is redder than noon.** The same thing carried to its end: when
//!   the Sun is on the horizon its light goes through the whole thickness, and
//!   no blue is left in it at all;
//! - **from orbit the air is a thin glowing arc**, not half the sky, and it
//!   **adds** light rather than replacing the background.
//!
//! Each of these statements catches its own bug. The first, swapped axes of the
//! sky table. The second, lost transmittance (without it a sunset would stay
//! white). The third, replacement instead of addition, i.e. a black arc on the
//! night edge.
//!
//! ## And one statement about what is not there
//!
//! A body without air gives the same frame as before stage S (rule 4). The
//! cheapest guard against "it ran through everything", and it is checked
//! directly: outside the planet's disc every pixel must equal the clear colour
//! **exactly**.

use engine::camera::Camera;
use engine::frame::{CLEAR_BYTES, LIGHT_DIR};
use engine::gpu::Gpu;
use engine::scene::{Atmosphere, Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::sphere::EARTH_RADIUS_M as EARTH;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sun_direction() -> [f64; 3] {
    unit(LIGHT_DIR.map(f64::from))
}

/// Earth with air or without it.
fn earth(air: bool) -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: EARTH,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: engine::frame::COLOUR,
        air: air.then(|| Atmosphere::EARTH.with_surface(EARTH)),
    }
}

/// An observer at altitude `altitude` above the point `up_dir`, looking
/// `elevation` degrees above the horizon **towards the Sun**.
///
/// The azimuth is towards the Sun, not arbitrary: the whole difference between
/// noon and sunset lives in it, and a camera turned off to the side would show
/// the same sky in both cases.
fn observer(up_dir: [f64; 3], altitude: f64, elevation: f64, air: bool) -> Scene {
    let up = unit(up_dir);
    let eye = up.map(|v| v * (EARTH + altitude));
    let sun = sun_direction();
    let mu_s = sun[0] * up[0] + sun[1] * up[1] + sun[2] * up[2];

    // The horizontal component of the direction to the Sun. At the subsolar
    // point it degenerates -- and then the azimuth does not matter, because the
    // Sun is at the zenith.
    let mut horizontal = [
        sun[0] - mu_s * up[0],
        sun[1] - mu_s * up[1],
        sun[2] - mu_s * up[2],
    ];
    if horizontal.iter().map(|v| v * v).sum::<f64>().sqrt() < 1.0e-6 {
        horizontal = cross(up, [0.0, 0.0, 1.0]);
    }
    let horizontal = unit(horizontal);

    let (sin, cos) = elevation.to_radians().sin_cos();
    let direction = [
        cos * horizontal[0] + sin * up[0],
        cos * horizontal[1] + sin * up[1],
        cos * horizontal[2] + sin * up[2],
    ];
    let target = [
        eye[0] + direction[0] * 1000.0,
        eye[1] + direction[1] * 1000.0,
        eye[2] + direction[2] * 1000.0,
    ];

    let mut scene = Scene::new(Camera::look_at(eye, target, up));
    scene.bodies.push(earth(air));
    scene
}

/// An observer at altitude `altitude` looking `depression` degrees **below** the
/// horizontal: the surface stretches from a few kilometres underfoot to the
/// horizon, i.e. the same ground is seen at every distance at once.
fn looking_down(altitude: f64, depression: f64, air: bool) -> Scene {
    let sun = sun_direction();
    let side = unit(cross(sun, [0.0, 0.0, 1.0]));
    // Neither at the subsolar point nor at the terminator: the surface is
    // brightly lit, but the Sun is not behind our back.
    let up = unit([sun[0] + side[0], sun[1] + side[1], sun[2] + side[2]]);
    let eye = up.map(|v| v * (EARTH + altitude));
    let forward = unit(cross(up, side));
    let (sin, cos) = (-depression.to_radians()).sin_cos();
    let direction = [
        cos * forward[0] + sin * up[0],
        cos * forward[1] + sin * up[1],
        cos * forward[2] + sin * up[2],
    ];
    let target = [
        eye[0] + direction[0] * 1.0e4,
        eye[1] + direction[1] * 1.0e4,
        eye[2] + direction[2] * 1.0e4,
    ];
    let mut scene = Scene::new(Camera::look_at(eye, target, up));
    scene.bodies.push(earth(air));
    scene
}

/// A view of the limb: the camera at altitude `altitude` above the terminator,
/// looking exactly at the horizon -- towards the Sun (`towards_sun`) or away
/// from it.
///
/// Above the **terminator**, because there the Sun is horizontal: the same view
/// forward gives a lit limb, the same one backwards a night one. Two scenes from
/// one number.
fn limb(altitude: f64, towards_sun: bool) -> Scene {
    let sun = sun_direction();
    let up = unit(cross(sun, [0.0, 0.0, 1.0]));
    let distance = EARTH + altitude;
    let eye = up.map(|v| v * distance);

    // The horizon dip angle from this altitude -- exactly, not approximately: it
    // is what puts the limb in the centre of the frame rather than somewhere.
    let (sin, cos) = (EARTH / distance).acos().sin_cos();
    let sign = if towards_sun { 1.0 } else { -1.0 };
    let direction = [
        cos * sun[0] * sign - sin * up[0],
        cos * sun[1] * sign - sin * up[1],
        cos * sun[2] * sign - sin * up[2],
    ];
    let target = [
        eye[0] + direction[0] * 1.0e6,
        eye[1] + direction[1] * 1.0e6,
        eye[2] + direction[2] * 1.0e6,
    ];

    let mut scene = Scene::new(Camera::look_at(eye, target, up));
    scene.bodies.push(earth(true));
    scene
}

/// The altitude at which a pixel's ray passes closest to the body's centre.
///
/// That is the altitude the limb's light belongs to: the ray is tangent, so its
/// whole path runs near it. Computed exactly -- `sqrt(|eye|^2 - dot(eye, w)^2)`
/// -- rather than through angles in the frame: the second way would have an
/// error of its own, and it would enter the measured scale height.
fn tangent_altitude(scene: &Scene, size: u32, column: u32, row: u32) -> f64 {
    let eye = scene.camera.position();
    let (right, up, forward) = scene.camera.axes();
    // The frame is square, so the tangent of the half-angle is the same on both
    // axes.
    let t = (engine::frame::FOV_Y / 2.0).tan();
    let ndc_x = 2.0 * (f64::from(column) + 0.5) / f64::from(size) - 1.0;
    let ndc_y = 1.0 - 2.0 * (f64::from(row) + 0.5) / f64::from(size);
    // The column enters on equal terms with the row, and that is not pedantry:
    // the limb in the frame is curved, so at the frame's edge the same row is
    // tangent to the layer considerably lower. A formula using the row alone
    // would give an altitude that pixel does not have.
    let w = unit([
        forward[0] + right[0] * ndc_x * t + up[0] * ndc_y * t,
        forward[1] + right[1] * ndc_x * t + up[1] * ndc_y * t,
        forward[2] + right[2] * ndc_x * t + up[2] * ndc_y * t,
    ]);
    let along = eye[0] * w[0] + eye[1] * w[1] + eye[2] * w[2];
    let radius = eye[0] * eye[0] + eye[1] * eye[1] + eye[2] * eye[2];
    (radius - along * along).max(0.0).sqrt() - EARTH
}

/// The planet wholly in frame, from an altitude of 1e7 m -- the same geometry as
/// in `--shot`.
fn from_orbit(air: bool) -> Scene {
    let eye = [EARTH + 1.0e7, 0.0, 0.0];
    let mut scene = Scene::new(Camera::look_at(eye, [0.0; 3], [0.0, 0.0, 1.0]));
    scene.bodies.push(earth(air));
    scene
}

fn render(gpu: &Gpu, scene: &Scene, name: &str) -> Shot {
    let shot = shot::take_scene(gpu, WIDTH, HEIGHT, scene).expect("the frame should have drawn");
    shot.write_png(std::path::Path::new(&format!("build/s4_{name}.png")))
        .expect("the screenshot should have been written");
    shot
}

/// The ratio of red to blue -- what "redder" is measured by.
///
/// WARNING: the bytes are decoded (T5a). The screenshot target encodes gamma, so
/// a ratio of bytes compresses the true ratio of luminances by a root of power
/// 2.4: a tenfold difference looks two-and-a-half-fold. The thresholds below are
/// measured in **linear** light, and that is where they mean the physics of
/// scattering.
fn redness(pixel: [u8; 4]) -> f64 {
    let red = engine::srgb::byte_to_linear(pixel[0]);
    let blue = engine::srgb::byte_to_linear(pixel[2]);
    red / blue.max(1.0 / 255.0 / 12.92)
}

fn centre(shot: &Shot) -> [u8; 4] {
    shot.pixel(WIDTH / 2, HEIGHT / 2)
}

/// The sky from the surface: brighter and whiter near the horizon than at the
/// zenith.
///
/// Both screenshots are from one point, the only difference being the view angle
/// -- so what is caught is the path length through the air rather than something
/// in the camera.
#[test]
fn the_sky_is_brighter_and_whiter_towards_the_horizon() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // Not exactly at the subsolar point: there the azimuth to the Sun is
    // degenerate, and an error in it would not show up at all.
    let sun = sun_direction();
    let side = unit(cross(sun, [0.0, 0.0, 1.0]));
    let noon = unit([
        sun[0] + 0.25 * side[0],
        sun[1] + 0.25 * side[1],
        sun[2] + 0.25 * side[2],
    ]);

    let zenith = centre(&render(
        &gpu,
        &observer(noon, 2.0, 89.0, true),
        "noon_zenith",
    ));
    let horizon = centre(&render(
        &gpu,
        &observer(noon, 2.0, 3.0, true),
        "noon_horizon",
    ));

    // There is a sky at all: the clear colour is [5, 8, 20], and the zenith must
    // be noticeably lighter than it, otherwise the pass drew nothing.
    assert!(
        zenith[2] > u32::from(CLEAR_BYTES[2]) as u8 * 2,
        "the zenith {zenith:?} is no lighter than the background {CLEAR_BYTES:?}"
    );

    // Measured: blue 89 at the zenith against 166 near the horizon, i.e. 1.86
    // times. The threshold of 1.3 leaves margin for a change of exposure or of
    // the march step.
    let brighter = f64::from(horizon[2]) / f64::from(zenith[2]);
    assert!(
        brighter > 1.3,
        "the sky near the horizon is not brighter: {horizon:?} against {zenith:?}"
    );

    // Measured: red/blue 0.28 at the zenith against 0.42 near the horizon. The
    // blue scatters away along the road while the red gets through -- hence the
    // whiteness of the horizon.
    assert!(
        redness(horizon) > redness(zenith) * 1.2,
        "the horizon is not whiter: {} against {}",
        redness(horizon),
        redness(zenith)
    );
}

/// A sunset is redder than noon, and not "a little".
///
/// Both cameras are on the surface and look at the same small angle towards the
/// Sun; the only difference is where the Sun is. Without transmittance along the
/// ray to the Sun a sunset would stay as white as noon -- that is exactly the bug
/// this test catches.
#[test]
fn a_sunset_is_redder_than_noon() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let sun = sun_direction();
    let side = unit(cross(sun, [0.0, 0.0, 1.0]));
    let noon = unit([
        sun[0] + 0.25 * side[0],
        sun[1] + 0.25 * side[1],
        sun[2] + 0.25 * side[2],
    ]);

    let at_noon = centre(&render(&gpu, &observer(noon, 2.0, 3.0, true), "noon_low"));
    // An observer on the terminator: the Sun is exactly on their horizon.
    let at_sunset = centre(&render(&gpu, &observer(side, 2.0, 3.0, true), "sunset"));

    // Measured: 0.42 at noon against 4.45 at sunset, i.e. tenfold. The threshold
    // of 5 is half the measured value.
    assert!(
        redness(at_sunset) > redness(at_noon) * 5.0,
        "the sunset {at_sunset:?} (r/b {}) is no redder than noon {at_noon:?} (r/b {})",
        redness(at_sunset),
        redness(at_noon)
    );
    // And it really is visible, not just a reddish zero.
    assert!(
        at_sunset[0] > 40,
        "the sunset is too dark to speak of: {at_sunset:?}"
    );
}

/// From orbit the air is a thin glowing arc, and it **adds** light.
///
/// Three numbers instead of the eye: the arc is there, it is thin, and not a
/// single pixel got darker from it. The last one catches replacement instead of
/// addition -- that is what gnawed a black arc out of the background on the night
/// edge, where there is nothing to scatter.
#[test]
fn from_orbit_the_air_is_a_thin_arc_that_only_adds_light() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let with_air = render(&gpu, &from_orbit(true), "orbit");
    let bare = render(&gpu, &from_orbit(false), "orbit_bare");

    let total = (WIDTH * HEIGHT) as usize;
    let mut changed = 0;
    let mut darker = 0;
    for k in 0..total {
        let a = &with_air.pixels[k * 4..k * 4 + 3];
        let b = &bare.pixels[k * 4..k * 4 + 3];
        // What is counted is **empty space**, not the whole frame: the planet's
        // disc is also changed by aerial perspective (S5), and that covers it
        // entirely -- a different statement and a different test.
        if b == CLEAR_BYTES && a != b {
            changed += 1;
        }
        // **Empty space only.** Where something is drawn, the air has every
        // right to darken it: aerial perspective (S5) multiplies the frame by
        // transmittance, and the planet's disc through a hundred kilometres of
        // air really is dimmer. Empty sky, though, the air only lights up -- there
        // it occludes nothing, and a pixel that got darker would mean replacement
        // instead of addition.
        if b == CLEAR_BYTES && a.iter().zip(b).any(|(x, y)| x < y) {
            darker += 1;
        }
    }

    assert_eq!(darker, 0, "{darker} pixels of empty sky got darker");
    // Measured: 852 pixels out of 230 400, i.e. 0.37% of the frame. A 100 km
    // layer on a radius of 6371 km from ten megametres is a band two or three
    // pixels wide along the disc, and that is exactly the order to expect.
    let share = changed as f64 / total as f64;
    assert!(
        (0.0005..0.05).contains(&share),
        "the air changed {share} of the frame -- that is no longer a thin arc"
    );
}

/// A body without air gives the same frame as before stage S.
///
/// Checked directly: outside the planet's disc every pixel equals the clear
/// colour **exactly**. A sky pass that ran one time too many would leave at
/// least a unit there -- adding zero comes for free only on paper.
#[test]
fn a_body_without_air_leaves_the_frame_exactly_as_it_was() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let bare = render(&gpu, &from_orbit(false), "orbit_bare");
    // The disc takes +-132 pixels from the centre (asin(R/(R+1e7)) = 22.9 deg),
    // so the left edge of the frame is definitely empty space.
    for y in (0..HEIGHT).step_by(17) {
        for x in (0..80).step_by(7) {
            let pixel = bare.pixel(x, y);
            assert_eq!(
                &pixel[..3],
                &CLEAR_BYTES,
                "pixel ({x}, {y}) outside the disc is {pixel:?}, but should have been the clear colour"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// S5 -- aerial perspective
// ---------------------------------------------------------------------------

/// The same ground at different distances: contrast falls, haze grows.
///
/// Both numbers come from one screenshot, and that matters -- the camera, the
/// lighting and the surface in it are the same everywhere, only the **distance**
/// differs: at the bottom of the frame the ground is eight kilometres away,
/// under the horizon seventy.
///
/// ## Why contrast is measured in red
///
/// Because the surface in the engine is still blue (`frame::COLOUR`), i.e.
/// almost the same hue as the haze. In blue the extinction and the in-scattering
/// nearly cancel each other, and the difference there is not about the air but
/// about a coincidence of two placeholders. In red they diverge the most -- that
/// is where what aerial perspective does becomes visible.
///
/// This is not fitting: the second test of the same screenshot is the **haze**,
/// i.e. the full difference from the air-free frame across all three channels.
/// It grows monotonically, and blue takes part in it on equal terms.
#[test]
fn the_same_ground_loses_contrast_and_gains_haze_with_distance() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let with_air = render(&gpu, &looking_down(5_000.0, 12.0, true), "ground_haze");
    let bare = render(&gpu, &looking_down(5_000.0, 12.0, false), "ground_bare");

    // The horizon is the first row from the top in which the surface appeared.
    // It is searched for rather than computed: it depends both on the altitude
    // and on the view angle, and computed a second time it would diverge from the
    // first.
    let column = WIDTH / 2;
    let horizon = (0..HEIGHT)
        .find(|&y| bare.pixel(column, y)[1] > 50)
        .expect("the surface should be in the frame");
    assert!(
        horizon > 20 && horizon < HEIGHT - 40,
        "the horizon is in row {horizon}"
    );
    // The sky slightly above the horizon -- what the surface turns into with
    // distance.
    let sky = with_air.pixel(column, horizon - 3);

    // Bottom-up, i.e. from near to far.
    let mut rows: Vec<u32> = (horizon + 4..HEIGHT - 4).step_by(12).collect();
    rows.reverse();
    assert!(rows.len() >= 6, "too few rows to compare: {}", rows.len());

    let mut previous_contrast = 1000;
    let mut previous_haze = -1000;
    let (mut first_contrast, mut last_contrast) = (0, 0);
    let (mut first_haze, mut last_haze) = (0, 0);
    for (index, &y) in rows.iter().enumerate() {
        let pixel = with_air.pixel(column, y);
        let plain = bare.pixel(column, y);
        let contrast = i32::from(pixel[0]) - i32::from(sky[0]);
        let contrast = contrast.abs();
        let haze: i32 = (0..3)
            .map(|c| (i32::from(pixel[c]) - i32::from(plain[c])).abs())
            .sum();

        // A tolerance of one unit is a single step of eight-bit colour, i.e. the
        // finest thing that can be written into a frame at all. Without it the
        // test would be catching rounding rather than physics.
        assert!(
            contrast <= previous_contrast + 1,
            "row {y}: contrast {contrast} against {previous_contrast} nearer -- with distance it should fall"
        );
        assert!(
            haze >= previous_haze - 2,
            "row {y}: haze {haze} against {previous_haze} nearer -- with distance it should grow"
        );
        previous_contrast = contrast;
        previous_haze = haze;
        if index == 0 {
            first_contrast = contrast;
            first_haze = haze;
        }
        last_contrast = contrast;
        last_haze = haze;
    }

    // And this is not "barely changed". Measured: contrast 63 -> 44, haze 6 -> 41
    // between eight kilometres and seventy.
    assert!(
        last_contrast * 4 < first_contrast * 3,
        "the contrast fell only from {first_contrast} to {last_contrast}"
    );
    assert!(
        last_haze > first_haze * 3,
        "the haze grew only from {first_haze} to {last_haze}"
    );
}

// ---------------------------------------------------------------------------
// S6 -- the limb and the planet's shadow
// ---------------------------------------------------------------------------

/// The glowing band at the edge of the disc falls off with the Rayleigh scale
/// height.
///
/// That is the step's oracle as named in ROADMAP-ATMOSPHERE.md: **the thickness
/// of the band against the scale height**, not "looks like a photo from orbit".
/// A ray tangent to the layer at altitude `h` runs almost its whole path near
/// that altitude, so in the transparent part of the atmosphere its brightness is
/// proportional to the density, i.e. `exp(-h/H)`. Hence the band's e-folding must
/// equal `H` -- 8 km, and no other number fits here.
///
/// It is measured in the **transparent** part, from 35 to 55 km. Lower down the
/// band is saturated: a tangent ray at ten kilometres has an optical depth of
/// order unity, and there brightness is no longer proportional to density.
/// Measured: in the transparent part the e-folding is 8.1 km, near the surface
/// 12.5 km, and the second number is saturation rather than different physics.
#[test]
fn the_limb_glow_falls_off_with_the_scale_height() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // 1600 pixels for the sake of resolution: a hundred-kilometre layer on the
    // limb from 500 km subtends 2.2 deg, i.e. about 54 rows at a 60 deg field of
    // view. Eight kilometres of scale height are four of those rows.
    const SIZE: u32 = 1600;
    let scene = limb(500_000.0, true);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("the frame should have drawn");
    shot.write_png(std::path::Path::new("build/s6_limb_day.png"))
        .expect("the screenshot should have been written");

    // The profile: tangent altitude against brightness above the background. Only
    // above the surface -- below the limb it is already the surface, not air.
    let mut profile: Vec<(f64, f64)> = Vec::new();
    for row in 0..SIZE {
        let altitude = tangent_altitude(&scene, SIZE, SIZE / 2, row);
        if altitude <= 5_000.0 || altitude > 120_000.0 {
            continue;
        }
        let blue = f64::from(shot.pixel(SIZE / 2, row)[2]) - f64::from(CLEAR_BYTES[2]);
        profile.push((altitude, blue.max(0.0)));
    }
    assert!(
        profile.len() > 30,
        "the profile is too small: {}",
        profile.len()
    );
    // Bottom-up.
    profile.sort_by(|a, b| a.0.total_cmp(&b.0));

    // The altitudes at which the brightness crosses two levels differing by
    // exactly tenfold. Ten times is `ln 10 = 2.303` e-foldings, so the distance
    // between them divided by that number gives the scale height.
    let crossing = |level: f64| -> Option<f64> {
        profile.windows(2).find_map(|pair| {
            let ((h0, v0), (h1, v1)) = (pair[0], pair[1]);
            (v0 >= level && v1 < level).then(|| h0 + (h1 - h0) * (v0 - level) / (v0 - v1))
        })
    };
    let high = crossing(50.0).expect("the band is nowhere brighter than 50");
    let low = crossing(5.0).expect("the band is nowhere dimmer than 5");
    assert!(
        low > high,
        "the brightness does not fall with altitude: {high} -> {low}"
    );

    let scale_height = (low - high) / 10.0f64.ln();
    let expected = f64::from(Atmosphere::EARTH.rayleigh_height_m);
    // Measured 8.1 km against 8.0 in the air's parameters. The tolerance of one
    // and a half times is not from uncertainty about the physics but because the
    // levels 50 and 5 are not strictly in the transparent part: the lower edge
    // pulls saturation upwards.
    assert!(
        scale_height > expected / 1.5 && scale_height < expected * 1.5,
        "the band's e-folding is {scale_height} m against a scale height of \
         {expected} m (crossings at {high} and {low} m)"
    );
}

/// The night side of the limb is dark: nothing glows above the surface.
///
/// Nobody draws the planet's shadow separately here -- it comes out by itself
/// from the ray to the Sun being tested for meeting the surface at every point
/// of air (S3). The test catches exactly the bug that would make that test
/// pointless: air lit through the planet.
#[test]
fn the_night_side_of_the_limb_does_not_glow() {
    let Some(gpu) = Gpu::for_tests() else { return };

    const SIZE: u32 = 800;
    let scene = limb(500_000.0, false);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("the frame should have drawn");
    shot.write_png(std::path::Path::new("build/s6_limb_night.png"))
        .expect("the screenshot should have been written");

    let mut checked = 0;
    for row in 0..SIZE {
        let altitude = tangent_altitude(&scene, SIZE, SIZE / 2, row);
        if !(1_000.0..100_000.0).contains(&altitude) {
            continue;
        }
        checked += 1;
        let pixel = shot.pixel(SIZE / 2, row);
        assert_eq!(
            &pixel[..3],
            &CLEAR_BYTES,
            "row {row} (altitude {altitude:.0} m): {pixel:?} -- the night air glows"
        );
    }
    assert!(
        checked > 10,
        "only {checked} rows of the layer were checked"
    );

    // And for contrast, the same limb from the same side but with the Sun: there
    // it does glow. Without this the test above would pass on a frame with
    // nothing in it.
    let day = shot::take_scene(&gpu, SIZE, SIZE, &limb(500_000.0, true))
        .expect("the frame should have drawn");
    let brightest = (0..SIZE)
        .filter(|&row| {
            (1_000.0..100_000.0).contains(&tangent_altitude(&scene, SIZE, SIZE / 2, row))
        })
        .map(|row| day.pixel(SIZE / 2, row)[2])
        .max()
        .expect("there are rows");
    assert!(
        brightest > CLEAR_BYTES[2] * 4,
        "the daytime limb does not glow either: {brightest}"
    );
}

/// One shader from the surface and from orbit: at the top of the air both paths
/// meet.
///
/// Rule 3 of stage S -- "one shader from the surface and from orbit" -- and this
/// is its sharpest check. A camera inside the air reads the sky table, a camera
/// outside it marches a ray; those are two different pipelines, and the boundary
/// between them is exactly the top of the atmosphere. A kilometre on either side
/// of it must give the same frame, otherwise a seam will flicker in the game at
/// that altitude.
///
/// Measured: **8 units out of 255**, i.e. 3%, and the reason for them is named.
/// It is not the march step -- raising it from 16 to 48 changes nothing at all;
/// it is the angular resolution of the sky table, in which near the horizon one
/// texel covers a noticeable arc. So the seam will not disappear from more
/// accurate integration, and it can only be reduced by a larger table -- which is
/// a question of cost, not of correctness.
#[test]
fn the_two_paths_meet_at_the_top_of_the_air() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // Ten metres on either side, not a kilometre, and that is not
    // over-caution. Cameras at different altitudes see the limb slightly
    // differently -- the horizon dip angle and the scale of altitudes in the
    // frame depend on it -- and at a kilometre that geometry contributes more
    // than the difference of paths ever could. At ten metres it vanishes: the
    // horizon shifts by four thousandths of a pixel.
    const SIZE: u32 = 320;
    let thickness = Atmosphere::EARTH_THICKNESS_M;
    let inside = shot::take_scene(&gpu, SIZE, SIZE, &limb(thickness - 10.0, true))
        .expect("the frame should have drawn");
    let outside = shot::take_scene(&gpu, SIZE, SIZE, &limb(thickness + 10.0, true))
        .expect("the frame should have drawn");

    // What is compared is the **sky**, not the whole frame: the rows in which the
    // ray passes through air above the surface. Below the limb the ground is
    // visible, and there both cameras draw it by the same path (aerial
    // perspective, S5) -- there is a difference of a few units, but it is about
    // the cameras really being at different altitudes rather than about the seam
    // between the paths. Here the seam is what is checked.
    let scene = limb(thickness, true);
    let mut worst = 0i32;
    let mut worst_at = (0u32, 0u32);
    let mut rows = 0;
    for row in 0..SIZE {
        if !(5_000.0..90_000.0).contains(&tangent_altitude(&scene, SIZE, SIZE / 2, row)) {
            continue;
        }
        rows += 1;
        for column in 0..SIZE {
            // The altitude comes from the pixel itself, not from the row: at the
            // frame's edge the limb is curved, and the same row there is already
            // in the surface.
            if !(5_000.0..90_000.0).contains(&tangent_altitude(&scene, SIZE, column, row)) {
                continue;
            }
            for c in 0..3 {
                let difference = (i32::from(inside.pixel(column, row)[c])
                    - i32::from(outside.pixel(column, row)[c]))
                .abs();
                if difference > worst {
                    worst = difference;
                    worst_at = (column, row);
                }
            }
        }
    }
    assert!(rows > 5, "only {rows} rows of sky were checked");
    assert!(
        worst <= 12,
        "a seam at the top of the air: {worst} units in pixel {worst_at:?}"
    );
}
