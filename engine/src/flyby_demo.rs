//! A flyby past the Moon on an elliptical orbit -- an animation (stage T
//! probe).
//!
//! The same genre as [`crate::ship_demo`] and [`crate::moon_demo`], and for
//! the same reason: to show, through the **very same** [`Frame`] that goes to
//! the window, what the stage has just made possible. Here it is shown all
//! together, because together it had not been visible anywhere yet:
//!
//! - surface colour from the LROC WAC mosaic and LOLA terrain right under the
//!   ship;
//! - the hull from Blender (T5d) with a GGX metal material (T5c);
//! - **the Moon's shine on the shadowed side of the hull** (T6): it changes
//!   both with altitude -- the disc form factor falls off as `sin^2(theta)` --
//!   and with whatever the ship is flying over, because the albedo comes from
//!   the asset;
//! - the tonemapper (T5c3): the highlight on the hull goes past one and
//!   without it would clip into a white blob;
//! - **two bodies in frame two orders of magnitude apart in distance**: the
//!   Moon a few thousand kilometres away and the Earth at 384 400 km (R1e and
//!   the depth ranges of V3).
//!
//!     cargo run --release -p engine -- --flyby-demo build/flyby.apng
//!
//! ## The composition was chosen by numbers, not by eye
//!
//! Three requirements: the Moon centred in frame, the Earth behind it, smooth
//! motion. Each one pins something down, and none can be met "approximately".
//!
//! **The Moon centred** means the camera looks at its centre rather than at
//! the ship, i.e. this is no longer [`crate::chase`]: a third-person camera
//! always keeps the vessel in the middle. The ship is offset by exactly
//! [`SHIP_OFF_AXIS`] -- the eye stands radially above it and a little to the
//! side, and the angle between that offset and the radius is the angle at
//! which the ship is seen from the centre of frame.
//!
//! **The Earth behind it is a requirement on the orbit, not on the camera.**
//! The tidally locked Moon faces the Earth with longitude 0, and the mosaic in
//! the asset reflects that; so a camera aimed at the Moon's centre sees the
//! Earth behind it only when the ship is flying over the **far** side. Hence
//! the line of apsides: apoapsis at longitude 180 deg - [`EARTH_MARGIN`], i.e.
//! 20 deg away from the anti-Earth point.
//!
//! WARNING: **From down low the Earth does not enter the frame at all, and
//! that is geometry, not a flaw in the composition.** The Moon's disc has an
//! angular radius of 63.7 deg at 200 km altitude -- wider than the whole frame
//! -- so with the camera aimed at the Moon's centre no sky is left outside the
//! disc. The measured track along one revolution:
//!
//! | altitude | Earth off axis | limb | what is in frame |
//! |---|---|---|---|
//! | 6000 km (apoapsis) | 19.7 deg | 13.0 deg | Earth clear of the limb |
//! | 5472 km | 9.6 deg | 13.9 deg | **occultation**: Earth behind the disc |
//! | 4280 km | 20.5 deg | 16.8 deg | Earth visible again |
//! | 3048 km | 37.1 deg | 21.3 deg | Earth past the edge of frame |
//! | 200 km (periapsis) | 159.8 deg | 63.7 deg | Earth behind the camera |
//!
//! So the Earth is in frame for roughly a quarter of the revolution -- the
//! high part of it -- and within that quarter it manages to **go behind the
//! limb and come back out**. The occultation here is real rather than a
//! departure from frame, and its oracle is the crossing of two angles.
//!
//! Periapsis falls on the near side (longitude -20 deg) -- where the mosaic
//! holds the maria, i.e. the greatest albedo contrast, which is what gives the
//! material rule (T4) and the shine (T6) something to show.
//!
//! **Smoothness is the choice of the parameter the frames are taken over.**
//! Uniformly in time the vessel nearly stands still near apoapsis; uniformly
//! in eccentric anomaly the angular rate doubles at periapsis (`dnu/dE` there
//! is `sqrt((1+e)/(1-e))`). Frames are taken uniformly in **true anomaly**:
//! then the direction to the ship from the Moon's centre creeps at a constant
//! angular rate. The camera's "up" is the constant orbit normal, so the motion
//! has neither jerks nor roll, and the loop closes without a jump.
//!
//! WARNING: **The animation must not be read as speed** -- time in it is
//! non-uniform by construction. The orbit itself is exact, though -- the
//! two-body problem has a closed form, and there is no integration here at
//! all.
//!
//! This is deliberately not `prop_run`. The probe shows a **frame**, and
//! taking an integrator for it would mean dragging the ephemeris, a moving
//! Moon and a choice of frame into the animation -- three things, none of
//! which affects the picture. The integrator's oracle lives separately
//! (`tests/live.rs`, `--live-probe`), and substituting an animation for it is
//! not allowed: an animation checks nothing.

use std::path::Path;

use crate::camera::Camera;
use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::scene::{Body, Scene, Ship, TerrainId, TileSet};
use crate::{demo, ship, ship_demo, shot, sphere, tiles};

/// Radius of the Moon, metres -- the same as in the other probes.
const RADIUS_M: f64 = 1_737_400.0;

/// Gravitational parameter of the Moon, m^3/s^2 (DE440).
const MU: f64 = 4.902_800_118e12;

/// Apoapsis and periapsis altitudes above the surface, metres.
const APOAPSIS_M: f64 = 6_000_000.0;
const PERIAPSIS_M: f64 = 200_000.0;

/// Mean distance to the Earth, metres.
const EARTH_RANGE_M: f64 = 384_400_000.0;

/// How far apoapsis is led away from the anti-Earth point, radians.
///
/// **A measured number, not taste.** The Earth has to be beyond the limb (the
/// disc's angular radius at apoapsis is 12.9 deg) and inside the camera's
/// vertical half-angle (30 deg). Twenty degrees leave margin on both sides --
/// and it is exactly that margin the descent eats up: at 3.3e6 m the disc
/// grows to 20 deg and the Earth hides.
const EARTH_MARGIN: f64 = 0.35;

/// Inclination of the orbit to the Moon's equator, radians.
///
/// Neither zero nor 90 deg: a polar orbit would pass over the poles, where the
/// WAC mosaic was shot at the worst angles, and an equatorial one would run
/// along a single belt. The rotation goes **about the line of apsides**, so
/// apoapsis and periapsis stay on the equator -- and the whole composition
/// rests on their longitudes.
const INCLINATION: f64 = 0.52;

/// Angle of the light source from periapsis, radians.
///
/// 70 deg, i.e. a low sun over the point of the lowest pass: that is where the
/// terrain casts the longest shadows. A light overhead would make the surface
/// flat.
const SOLAR_ZENITH: f64 = 1.22;

/// How many hull extents from the camera to the ship.
const RANGES: f64 = 3.2;

/// The angle by which the ship is led away from the centre of frame, radians.
///
/// The centre of frame is taken by the Moon, so the ship has to stand aside --
/// but inside the frame: 0.28 rad is 16 deg, a little over half the camera's
/// half-angle.
const SHIP_OFF_AXIS: f64 = 0.28;

/// A minute of video: exactly as many frames as a minute holds at [`FPS`].
///
/// One full revolution per animation, i.e. 8.39 hours of orbital time in sixty
/// seconds. Fewer frames would give the same trajectory faster -- that is
/// `--frames`.
pub const FRAMES: u32 = 3600;
pub const FPS: u16 = 60;

/// Semi-major axis and eccentricity from the two altitudes.
fn elements() -> (f64, f64) {
    let apo = RADIUS_M + APOAPSIS_M;
    let peri = RADIUS_M + PERIAPSIS_M;
    (0.5 * (apo + peri), (apo - peri) / (apo + peri))
}

/// Eccentric anomaly from the true one -- exact form, no iteration.
fn eccentric_from_true(true_anomaly: f64) -> f64 {
    let (_, e) = elements();
    2.0 * (((1.0 - e) / (1.0 + e)).sqrt() * (0.5 * true_anomaly).tan()).atan()
}

/// State at an eccentric anomaly: position and velocity, world axes.
///
/// The closed form of the two-body problem: the perifocal plane, a rotation by
/// the inclination about the line of apsides, a rotation of the whole orbit
/// about the polar axis by [`EARTH_MARGIN`]. The velocity is needed not by the
/// physics but by the **orientation**: the ship's nose looks along it.
fn state_at(e_anomaly: f64) -> ([f64; 3], [f64; 3]) {
    let (a, e) = elements();
    let (sin_e, cos_e) = e_anomaly.sin_cos();

    // Perifocal frame: periapsis on the `+x` axis.
    let r = a * (1.0 - e * cos_e);
    let plane = [a * (cos_e - e), a * (1.0 - e * e).sqrt() * sin_e];

    // Derivative of that same parametrisation: `dE/dt = n*a/r`, where
    // `n = sqrt(mu/a^3)`.
    let n = (MU / (a * a * a)).sqrt();
    let rate = n * a / r;
    let speed = [-a * sin_e * rate, a * (1.0 - e * e).sqrt() * cos_e * rate];

    // The inclination goes about `x`, i.e. about the line of apsides: apoapsis
    // and periapsis stay on the equator, and the composition rests on their
    // longitudes.
    let (sin_i, cos_i) = INCLINATION.sin_cos();
    let lift = |v: [f64; 2]| [v[0], v[1] * cos_i, v[1] * sin_i];

    // And a rotation of the whole orbit about `z`: periapsis moves to
    // longitude `-EARTH_MARGIN`, apoapsis to `180 deg - EARTH_MARGIN`.
    let (sin_arg, cos_arg) = (-EARTH_MARGIN).sin_cos();
    let turn = |v: [f64; 3]| {
        [
            v[0] * cos_arg - v[1] * sin_arg,
            v[0] * sin_arg + v[1] * cos_arg,
            v[2],
        ]
    };
    (turn(lift(plane)), turn(lift(speed)))
}

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

/// The orbit plane's normal -- constant, and that is exactly why the camera
/// does not wobble.
fn orbit_normal() -> [f64; 3] {
    let (position, velocity) = state_at(0.3);
    unit(cross(position, velocity))
}

/// Where the Earth stands as seen from the Moon.
///
/// Longitude 0: the tidally locked Moon faces the Earth with precisely that
/// one, and the mosaic in the asset reflects it. So the direction is not
/// chosen but dictated by what already lies in the tiles.
fn earth_centre() -> [f64; 3] {
    [EARTH_RANGE_M, 0.0, 0.0]
}

/// The direction to the light source -- derived from the orbit, not set by
/// eye.
///
/// Taken in the orbit plane at [`SOLAR_ZENITH`] from periapsis, on the
/// **approach** side: the ship descends over the lit surface, passes periapsis
/// under a low light, and then goes on towards the terminator, past which the
/// shine on the hull dies out (T6).
fn sun() -> [f64; 3] {
    let (periapsis, ahead) = state_at(0.0);
    let p = unit(periapsis);
    let a = unit(ahead);
    let (sin_z, cos_z) = SOLAR_ZENITH.sin_cos();
    unit([
        cos_z * p[0] - sin_z * a[0],
        cos_z * p[1] - sin_z * a[1],
        cos_z * p[2] - sin_z * a[2],
    ])
}

/// The quaternion `[w, x, y, z]` taking the ship's `+Z` to `forward` and the
/// ship's `+Y` as close to `up` as it can.
fn look_along(forward: [f64; 3], up: [f64; 3]) -> [f64; 4] {
    let z = unit(forward);
    let x = unit(cross(up, z));
    let y = cross(z, x);

    // The matrix columns are the images of the ship's axes in the world; then
    // the standard matrix-to-quaternion conversion through the largest trace.
    let m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]];
    let trace = m[0][0] + m[1][1] + m[2][2];
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [
            0.25 / s,
            (m[2][1] - m[1][2]) * s,
            (m[0][2] - m[2][0]) * s,
            (m[1][0] - m[0][1]) * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
        [
            (m[2][1] - m[1][2]) / s,
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
        [
            (m[0][2] - m[2][0]) / s,
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
        ]
    } else {
        let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
        [
            (m[1][0] - m[0][1]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
        ]
    }
}

/// The scene for frame number `k` out of `frames`.
///
/// Built from scratch every frame on purpose: a scene is data, and a probe
/// that held one between frames would be checking its cache rather than the
/// frame.
pub fn scene_at(k: u32, frames: u32, tiles: TileSet, earth: TileSet, extent: f64) -> Scene {
    let t = f64::from(k) / f64::from(frames.max(2));

    // Uniformly in **true** anomaly: from apoapsis through periapsis and back.
    // That is the one giving a constant angular rate in frame, i.e. smoothness.
    let true_anomaly = std::f64::consts::PI * (2.0 * t - 1.0);
    let (position, velocity) = state_at(eccentric_from_true(true_anomaly));

    let up = unit(position);
    let normal = orbit_normal();
    let ship = Ship {
        centre: position,
        orientation: look_along(velocity, up),
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: extent * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    };

    // The eye sits above the ship and a little off the orbit plane. The angle
    // between that offset and the radius equals the angle at which the ship is
    // seen from the centre of frame: the camera looks at the Moon's centre,
    // and the ship lies exactly back along the offset from it.
    let (sin_off, cos_off) = SHIP_OFF_AXIS.sin_cos();
    let offset = [
        cos_off * up[0] + sin_off * normal[0],
        cos_off * up[1] + sin_off * normal[1],
        cos_off * up[2] + sin_off * normal[2],
    ];
    let range = RANGES * ship.extent_m;
    let eye = [
        position[0] + range * offset[0],
        position[1] + range * offset[1],
        position[2] + range * offset[2],
    ];
    // The frame's "up" is the orbit normal, constant for the whole animation:
    // any up derived from the position would roll the frame along with the
    // motion.
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], normal);

    let mut scene = Scene::new(camera);
    scene.sun = sun();
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        // Grey rather than the blue of the fixtures: without a colour asset
        // the Moon still has to look like the Moon.
        colour: [0.55, 0.55, 0.56, 1.0],
        air: None,
    });
    // The Earth is the scene's second body, two orders of magnitude further
    // out. Smooth and without air: from that distance the disc is 1.9 deg, and
    // neither terrain nor a layer of air has anywhere to show in it (the S5
    // condition would have skipped the air anyway).
    scene.bodies.push(Body {
        centre: earth_centre(),
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: earth,
        colour: frame::COLOUR,
        air: None,
    });
    scene.ships.push(ship);
    scene
}

/// Draws `frames` frames and assembles them into an animated PNG.
pub fn render(gpu: &Gpu, width: u32, height: u32, frames: u32, path: &Path) -> Result<(), String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let surface = load_surface(gpu, &mut frame)?;
    let earth = match load_earth(gpu, &mut frame) {
        Some(id) => TileSet::Loaded(id),
        None => TileSet::Smooth,
    };

    // The hull from the asset if it has been cooked; otherwise the V1 stub.
    let hull = ship_demo::hull();
    if let Some(model) = &hull {
        frame.load_ship(gpu, model);
    }
    let extent = hull.as_ref().map_or(ship_demo::STUB_EXTENT, |m| m.extent);

    report();

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("flyby demo"),
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

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .set_animated(frames, 0)
        .map_err(|e| format!("APNG: {e}"))?;
    encoder
        .set_frame_delay(1, FPS)
        .map_err(|e| format!("APNG: {e}"))?;
    let mut writer = encoder.write_header().map_err(|e| format!("APNG: {e}"))?;

    for k in 0..frames {
        let scene = scene_at(k, frames, TileSet::Loaded(surface), earth, extent);
        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("flyby demo"),
            });
        frame.draw(gpu, &mut commands, &view, width, height, &scene);
        let shot = shot::read_back(gpu, commands, &texture, width, height)?;
        writer
            .write_image_data(&shot.pixels)
            .map_err(|e| format!("APNG: {e}"))?;
    }

    writer.finish().map_err(|e| format!("APNG: {e}"))?;
    Ok(())
}

/// Prints the orbital elements and the composition angles -- so that a number
/// in the frame can be checked against a number.
fn report() {
    let (a, e) = elements();
    let period = 2.0 * std::f64::consts::PI * (a * a * a / MU).sqrt();
    let speed = |r: f64| (MU * (2.0 / r - 1.0 / a)).sqrt();
    println!(
        "orbit: {:.0} x {:.0} km above the surface, a = {:.1} km, e = {:.4}, inclination {:.0} deg",
        PERIAPSIS_M / 1000.0,
        APOAPSIS_M / 1000.0,
        a / 1000.0,
        e,
        INCLINATION.to_degrees()
    );
    println!(
        "  period {:.2} h; speed {:.0} m/s at periapsis, {:.0} m/s at apoapsis",
        period / 3600.0,
        speed(RADIUS_M + PERIAPSIS_M),
        speed(RADIUS_M + APOAPSIS_M)
    );
    println!(
        "  Earth {:.0} deg off the view axis; Moon disc {:.1} deg at apoapsis, {:.1} deg at periapsis",
        EARTH_MARGIN.to_degrees(),
        (RADIUS_M / (RADIUS_M + APOAPSIS_M)).asin().to_degrees(),
        (RADIUS_M / (RADIUS_M + PERIAPSIS_M)).asin().to_degrees()
    );
}

/// The Moon's terrain and colour from the cooked assets.
fn load_surface(gpu: &Gpu, frame: &mut Frame) -> Result<TerrainId, String> {
    let bytes = std::fs::read(demo::TERRAIN_ASSET)
        .map_err(|e| format!("{}: {e}\nto fix: make cook-dem", demo::TERRAIN_ASSET))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;

    let bytes = std::fs::read(demo::COLOUR_ASSET)
        .map_err(|e| format!("{}: {e}\nto fix: make cook-colour", demo::COLOUR_ASSET))?;
    let colour = tiles::Colour::from_bytes(&bytes)?;
    frame.load_surface(gpu, &terrain, Some(&colour))
}

/// The Earth's surface -- the **second** tiled body in one frame (T7g).
///
/// Silently returns `None` when the asset is missing: it lives outside git
/// (Q5), and the probe has to draw without it too -- the Earth then stays a
/// smooth ball, as it was before this step. The same leniency as in
/// `game::app::load_surface`, and for the same reason: a missing asset is not
/// a broken engine.
fn load_earth(gpu: &Gpu, frame: &mut Frame) -> Option<TerrainId> {
    let terrain = tiles::Terrain::from_bytes(&std::fs::read(EARTH_TERRAIN_ASSET).ok()?).ok()?;
    let colour = tiles::Colour::from_bytes(&std::fs::read(EARTH_COLOUR_ASSET).ok()?).ok()?;
    frame.load_surface(gpu, &terrain, Some(&colour)).ok()
}

/// The cooked surface of the Earth (T7d, T7e).
const EARTH_TERRAIN_ASSET: &str = "assets/earth.dem";
const EARTH_COLOUR_ASSET: &str = "assets/earth.col";

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// The angle between two directions, radians.
    fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
        dot(unit(a), unit(b)).clamp(-1.0, 1.0).acos()
    }

    /// The apoapsis and periapsis altitudes are the ones ordered.
    #[test]
    fn the_orbit_has_the_two_altitudes_it_promises() {
        let radius = |e_anomaly: f64| {
            let (p, _) = state_at(e_anomaly);
            (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
        };
        let peri = radius(0.0) - RADIUS_M;
        let apo = radius(std::f64::consts::PI) - RADIUS_M;
        println!("  periapsis {peri:.1} m, apoapsis {apo:.1} m");
        assert!((peri - PERIAPSIS_M).abs() < 1.0);
        assert!((apo - APOAPSIS_M).abs() < 1.0);
    }

    /// The velocity really is the derivative of the position, not a separate
    /// formula.
    ///
    /// Two independent roads to one number: the closed form against a central
    /// difference in time. They have to meet, otherwise the ship's nose looks
    /// somewhere other than where it flies.
    #[test]
    fn the_velocity_is_the_derivative_of_the_position() {
        let (a, e) = elements();
        let n = (MU / (a * a * a)).sqrt();
        let time = |anomaly: f64| (anomaly - e * anomaly.sin()) / n;
        let anomaly_at = |t: f64| {
            let mut anomaly = n * t;
            for _ in 0..64 {
                let f = anomaly - e * anomaly.sin() - n * t;
                anomaly -= f / (1.0 - e * anomaly.cos());
            }
            anomaly
        };

        for probe in [0.3, 1.1, 2.4, 3.0] {
            let t0 = time(probe);
            let step = 1.0e-3;
            let (before, _) = state_at(anomaly_at(t0 - step));
            let (after, _) = state_at(anomaly_at(t0 + step));
            let (_, velocity) = state_at(probe);
            for k in 0..3 {
                let numeric = (after[k] - before[k]) / (2.0 * step);
                assert!(
                    (numeric - velocity[k]).abs() < 1e-3 * velocity[k].abs().max(1.0),
                    "anomaly {probe}, axis {k}: {numeric} vs {}",
                    velocity[k]
                );
            }
        }
    }

    /// The ship's nose looks along the velocity.
    #[test]
    fn the_ship_points_where_it_flies() {
        for k in [0u32, 37, 180, 300] {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            let ship = &scene.ships[0];
            let true_anomaly =
                std::f64::consts::PI * (2.0 * f64::from(k) / f64::from(FRAMES) - 1.0);
            let (_, velocity) = state_at(eccentric_from_true(true_anomaly));
            let r = crate::frame::rotation(ship.orientation);
            // The image of the ship's `+Z` is the third column of the rotation
            // matrix.
            let nose = [r[0][2], r[1][2], r[2][2]];
            let along = dot(nose, unit(velocity));
            println!("  frame {k}: nose along the velocity to {along:.6}");
            assert!(
                along > 0.999_999,
                "the nose does not look along the velocity"
            );
        }
    }

    /// The Moon is in the middle of the frame and the ship right beside it.
    ///
    /// Two claims in one number: the body's centre lies on the view axis, and
    /// the ship is [`SHIP_OFF_AXIS`] away from it, i.e. inside the frame and
    /// not on top of the centre.
    #[test]
    fn the_moon_is_in_the_middle_and_the_ship_beside_it() {
        for k in [0u32, 600, 1200, 1800, 2400, 3000] {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            let eye = scene.camera.position();
            let to_moon = [-eye[0], -eye[1], -eye[2]];
            let to_ship = {
                let c = scene.ships[0].centre;
                [c[0] - eye[0], c[1] - eye[1], c[2] - eye[2]]
            };
            let off = angle(to_moon, to_ship);
            println!(
                "  frame {k}: ship {:.2} deg off the centre",
                off.to_degrees()
            );
            assert!(
                (off - SHIP_OFF_AXIS).abs() < 0.02,
                "the ship left its place: {off} vs {SHIP_OFF_AXIS}"
            );
            // The frame's vertical half-angle is 30 deg, and the ship has to be
            // inside it.
            assert!(off < 0.5 * frame::FOV_Y);
        }
    }

    /// The Earth clears the limb at apoapsis, hides behind the disc shortly
    /// after it, and leaves the frame on the descent.
    ///
    /// Three different causes, and the test tells them apart rather than
    /// settling for "the Earth is somewhere": **behind the disc** -- when the
    /// angle to it is smaller than the limb's angular radius; **out of frame**
    /// -- when it is larger than the camera's half-angle; **visible** --
    /// between the two. The last check is what records the geometric limit of
    /// the composition: down low the disc is wider than the frame, so no sky
    /// is left outside it at all.
    #[test]
    fn the_earth_clears_the_limb_high_up_and_hides_behind_it_low_down() {
        let separation = |k: u32| {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            let eye = scene.camera.position();
            let to_moon = [-eye[0], -eye[1], -eye[2]];
            let earth = earth_centre();
            let to_earth = [earth[0] - eye[0], earth[1] - eye[1], earth[2] - eye[2]];
            let distance = (eye[0] * eye[0] + eye[1] * eye[1] + eye[2] * eye[2]).sqrt();
            (angle(to_moon, to_earth), (RADIUS_M / distance).asin())
        };

        // Apoapsis: the Earth is clear of the limb and inside the frame.
        let (apart, limb) = separation(0);
        println!(
            "  apoapsis: Earth at {:.1} deg, limb at {:.1} deg",
            apart.to_degrees(),
            limb.to_degrees()
        );
        assert!(
            apart > limb,
            "at apoapsis the Earth is behind the Moon's disc"
        );
        assert!(
            apart < 0.5 * frame::FOV_Y,
            "at apoapsis the Earth is out of frame"
        );

        // Shortly after apoapsis the track passes under the Earth -- an
        // occultation.
        let (apart, limb) = separation(180);
        println!(
            "  frame 180: Earth at {:.1} deg, limb at {:.1} deg",
            apart.to_degrees(),
            limb.to_degrees()
        );
        assert!(
            apart < limb,
            "the occultation is gone -- the Earth never went behind the disc"
        );

        // Periapsis: the Earth is behind the camera, because the camera looks
        // at the Moon's centre while the ship is already over the near side.
        let (apart, limb) = separation(FRAMES / 2);
        println!(
            "  periapsis: Earth at {:.1} deg, limb at {:.1} deg",
            apart.to_degrees(),
            limb.to_degrees()
        );
        assert!(
            apart > 0.5 * frame::FOV_Y,
            "at periapsis the Earth cannot be in frame"
        );
        // And this is not a mere "did not fit": the disc is wider than the
        // camera's half-angle, i.e. there is no sky outside it in frame at all.
        assert!(
            limb > 0.5 * frame::FOV_Y,
            "the limb should have covered the whole frame"
        );
    }

    /// The motion is smooth: the camera's angular step between frames does not
    /// wander.
    ///
    /// A numeric oracle, not "looks good". Uniformity in **true** anomaly is
    /// exactly what gives that: in eccentric anomaly the ratio of the largest
    /// step to the smallest would be `sqrt((1+e)/(1-e))` ~ 2, and in time it
    /// would be orders of magnitude. The last step is checked separately: the
    /// loop has to close.
    #[test]
    fn the_camera_moves_without_jerks() {
        let direction = |k: u32| {
            let scene = scene_at(k % FRAMES, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            unit(scene.camera.position())
        };
        let mut steps = Vec::with_capacity(FRAMES as usize);
        for k in 0..FRAMES {
            steps.push(angle(direction(k), direction(k + 1)));
        }
        let smallest = steps.iter().copied().fold(f64::INFINITY, f64::min);
        let largest = steps.iter().copied().fold(0.0, f64::max);
        println!(
            "  camera step: {:.4} deg ... {:.4} deg, ratio {:.4}",
            smallest.to_degrees(),
            largest.to_degrees(),
            largest / smallest
        );
        assert!(
            largest / smallest < 1.05,
            "the angular rate wanders by a factor of {:.2}",
            largest / smallest
        );
        assert!(
            steps[FRAMES as usize - 1] < 1.05 * smallest,
            "the loop has a gap"
        );
    }
}
