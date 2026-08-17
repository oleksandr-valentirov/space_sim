//! The orbit camera: what the player moves the view with (ROADMAP I2).
//!
//! The state is three `double`s: two angles and an altitude above the surface.
//! The camera position is **derived** from them rather than accumulated, and
//! that is the module's main decision. A camera that integrates offsets into
//! its own position drifts off the sphere over time, and there is no single
//! moment where that becomes noticeable.
//!
//! There is no GPU, no window and no `winit` here: numbers in, a [`Camera`]
//! out. So everything the module promises is checked without an adapter -- and
//! window events stay a thin layer in `app` that only translates them into
//! these calls.

use crate::camera::Camera;
use crate::sphere;

/// Lowest altitude, metres. The same as the closest point of the F5 flyby:
/// below that a 64x128 mesh is no longer a sphere but a single facet under the
/// camera's nose, and there is nothing to measure on it.
pub const MIN_ALTITUDE_M: f64 = 10.0;

/// Highest. 1e11 m is about 0.7 AU -- at that distance F4 measured that
/// camera-relative holds to the last digit; beyond it nothing is checked, so
/// the camera does not go there.
pub const MAX_ALTITUDE_M: f64 = 1.0e11;

/// Radians per pixel of mouse drag. Half a screen (~600 px) per half turn --
/// the pace orbit cameras usually have.
const RADIANS_PER_PIXEL: f64 = std::f64::consts::PI / 600.0;

/// The factor by which one wheel notch changes the altitude.
///
/// Geometric, not additive: from 10 m to 1e11 m is ten orders of magnitude,
/// and any constant step in metres is either motionless at one end or useless
/// at the other.
const ZOOM_PER_NOTCH: f64 = 1.25;

/// Where the pitch must not reach.
///
/// Exactly at the pole the view direction coincides with the "up" reference,
/// their cross product is zero, and the camera basis turns into NaN. Not a
/// theoretical risk: a user drags the camera to the pole within a second.
const PITCH_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - 1.0e-3;

pub struct Orbit {
    /// Azimuth about the z axis.
    yaw: f64,
    /// Elevation above the xy plane, bounded by [`PITCH_LIMIT`].
    pitch: f64,
    /// Altitude above the surface, metres.
    altitude: f64,
    /// Radius of the body the altitude is measured above, metres.
    ///
    /// A field rather than Earth's radius inline, because "10 km up" is a
    /// statement about a body, not about the camera. With Earth's radius
    /// hard-coded, the same call over the Moon put the camera 4634 km up --
    /// and nothing in the caller looked wrong. See [`Orbit::around`].
    reference_m: f64,
}

impl Default for Orbit {
    /// The same view as [`crate::frame::default_camera`] -- the one where the
    /// measured frame coverage is compared against the analytic formula. The
    /// interactive frame starts exactly where it is checked.
    fn default() -> Self {
        Orbit {
            yaw: 0.0,
            pitch: 0.0,
            altitude: crate::frame::DEFAULT_ALTITUDE_M,
            reference_m: sphere::EARTH_RADIUS_M,
        }
    }
}

impl Orbit {
    /// The same view, but from a different altitude.
    ///
    /// Needed by whoever draws something other than the planet: a halo orbit
    /// near L2 lies 4.5e8 m from Earth, and at the default altitude (1e7 m) it
    /// is not in the frame at all. The altitude is clamped by the same bounds
    /// as the wheel -- otherwise this would be a way around them rather than a
    /// constructor.
    pub fn at_altitude(altitude: f64) -> Orbit {
        Orbit::around(sphere::EARTH_RADIUS_M, altitude)
    }

    /// The same, above a body of the given radius.
    ///
    /// Everything the game looks at today is Earth, so [`at_altitude`] covers
    /// every caller in `game`. This one exists for whoever looks at something
    /// else -- the Moon, a fixture body -- and it exists because the mistake it
    /// prevents is silent: an altitude read against the wrong radius produces a
    /// perfectly valid camera at the wrong place, and the picture from it looks
    /// like an answer.
    ///
    /// [`at_altitude`]: Orbit::at_altitude
    pub fn around(reference_m: f64, altitude: f64) -> Orbit {
        Orbit {
            altitude: altitude.clamp(MIN_ALTITUDE_M, MAX_ALTITUDE_M),
            reference_m,
            ..Orbit::default()
        }
    }

    pub fn altitude(&self) -> f64 {
        self.altitude
    }

    /// Distance from the centre of the body the camera orbits.
    pub fn distance(&self) -> f64 {
        self.reference_m + self.altitude
    }

    /// A mouse drag of `dx`, `dy` pixels.
    pub fn drag(&mut self, dx: f64, dy: f64) {
        self.yaw += dx * RADIANS_PER_PIXEL;
        self.pitch = (self.pitch + dy * RADIANS_PER_PIXEL).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// `notches` wheel clicks: positive zooms in.
    pub fn zoom(&mut self, notches: f64) {
        let factor = ZOOM_PER_NOTCH.powf(-notches);
        self.altitude = (self.altitude * factor).clamp(MIN_ALTITUDE_M, MAX_ALTITUDE_M);
    }

    /// What the altitude is measured above, changed without moving the camera
    /// otherwise.
    ///
    /// Its own method rather than a new `Orbit`, because the yaw and the pitch
    /// are the player's and must survive: rebuilding through [`around`] would
    /// snap the view back to the default direction every time the target
    /// changed, which reads as the camera jumping rather than as the target
    /// changing.
    ///
    /// [`around`]: Orbit::around
    pub fn set_reference(&mut self, reference_m: f64) {
        self.reference_m = reference_m;
    }

    pub fn camera(&self) -> Camera {
        self.camera_about([0.0, 0.0, 0.0])
    }

    /// The same camera, orbiting a point that is not the origin.
    ///
    /// The scene the game builds is geocentric (`game::view`), so the origin is
    /// Earth and [`camera`] can only ever look at it. Everything else -- the
    /// Moon above all -- needs its centre passed in, and passed in **per
    /// frame**: a body moves, and a centre captured once would leave the camera
    /// aiming at where the Moon used to be.
    ///
    /// [`camera`]: Orbit::camera
    pub fn camera_about(&self, centre: [f64; 3]) -> Camera {
        let distance = self.distance();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();

        let position = [
            centre[0] + distance * cos_pitch * cos_yaw,
            centre[1] + distance * cos_pitch * sin_yaw,
            centre[2] + distance * sin_pitch,
        ];

        Camera::look_at(position, centre, [0.0, 0.0, 1.0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The planet stays dead ahead at any angles.
    ///
    /// This checks the whole chain angles -> position -> camera basis at once:
    /// the world centre must land on the view axis and its distance must match
    /// `distance()`. A sign error, a wrong multiplication order or swapped
    /// sin/cos would push the centre aside.
    #[test]
    fn the_planet_stays_dead_ahead() {
        for yaw_steps in 0..8 {
            for pitch_steps in -3..=3 {
                let mut orbit = Orbit::default();
                orbit.drag(f64::from(yaw_steps) * 100.0, f64::from(pitch_steps) * 100.0);

                let centre = orbit.camera().relative([0.0, 0.0, 0.0]);
                let distance = orbit.distance();

                assert!(
                    centre[0].abs() < 1.0 && centre[1].abs() < 1.0,
                    "the planet centre drifted aside: {centre:?}"
                );
                assert!(
                    ((-f64::from(centre[2])) - distance).abs() / distance < 1e-6,
                    "distance to the centre {} against the expected {distance}",
                    -f64::from(centre[2])
                );
            }
        }
    }

    /// The chosen centre stays dead ahead, wherever it is.
    ///
    /// The same claim as above and deliberately the same shape, because the
    /// mistake it guards against is the one that looks right: a camera moved to
    /// the body while still **aiming at the origin** gives a picture of the
    /// Moon's neighbourhood with the Moon off to one side, which reads as a
    /// misaligned scene rather than as a camera bug. So both halves are
    /// checked -- the distance from the centre, and the centre on the axis.
    ///
    /// The centre is off every axis of symmetry on purpose: with `[d, 0, 0]` a
    /// forgotten component would pass.
    #[test]
    fn the_chosen_centre_stays_dead_ahead() {
        let moon = [3.6e8, -1.2e8, 4.0e7];

        for yaw_steps in 0..8 {
            for pitch_steps in -3..=3 {
                let mut orbit = Orbit::default();
                orbit.drag(f64::from(yaw_steps) * 100.0, f64::from(pitch_steps) * 100.0);

                let camera = orbit.camera_about(moon);
                let centre = camera.relative(moon);
                let distance = orbit.distance();

                assert!(
                    centre[0].abs() < 1.0 && centre[1].abs() < 1.0,
                    "the centre drifted aside: {centre:?}"
                );
                assert!(
                    ((-f64::from(centre[2])) - distance).abs() / distance < 1e-6,
                    "distance to the centre {} against the expected {distance}",
                    -f64::from(centre[2])
                );
            }
        }
    }

    /// Orbiting the origin is orbiting a centre of zero, not a second
    /// implementation.
    ///
    /// Cheap, and it is what keeps the old camera from drifting away from the
    /// new one: every existing oracle in the engine stands on `camera()`.
    #[test]
    fn the_origin_is_just_a_centre_of_zero() {
        let mut orbit = Orbit::default();
        orbit.drag(137.0, -42.0);

        let point = [1.0e7, 2.0e7, -3.0e6];
        assert_eq!(
            orbit.camera().relative(point),
            orbit.camera_about([0.0, 0.0, 0.0]).relative(point),
            "the origin camera and a zero centre must agree bitwise"
        );
    }

    /// Changing what the altitude is measured above keeps the direction the
    /// player dragged to.
    #[test]
    fn retargeting_keeps_the_players_angles() {
        let mut orbit = Orbit::default();
        orbit.drag(220.0, 90.0);
        let before = orbit.camera().relative([0.0, 0.0, 0.0]);

        orbit.set_reference(1.7374e6);

        let after = orbit.camera().relative([0.0, 0.0, 0.0]);
        assert!(
            after[0].abs() < 1.0 && after[1].abs() < 1.0,
            "the centre left the axis after retargeting: {after:?}"
        );
        assert!(
            before[2] != after[2],
            "a smaller reference radius must bring the camera closer"
        );
    }

    /// Past the pole the camera neither flips over nor turns into NaN.
    ///
    /// The cheapest way to get a NaN in the engine is to bring the view
    /// exactly along the "up" reference. A user does that in a second of
    /// dragging.
    #[test]
    fn dragging_past_the_pole_stays_finite() {
        let mut orbit = Orbit::default();
        for _ in 0..100 {
            orbit.drag(0.0, 1000.0);
        }

        let p = orbit.camera().relative([0.0, 0.0, 0.0]);
        assert!(
            p.iter().all(|v| v.is_finite()),
            "the camera produced NaN: {p:?}"
        );
        assert!(orbit.pitch < std::f64::consts::FRAC_PI_2);

        for _ in 0..200 {
            orbit.drag(0.0, -1000.0);
        }
        let p = orbit.camera().relative([0.0, 0.0, 0.0]);
        assert!(
            p.iter().all(|v| v.is_finite()),
            "the camera produced NaN: {p:?}"
        );
        assert!(orbit.pitch > -std::f64::consts::FRAC_PI_2);
    }

    /// The camera cannot come closer to the planet than the surface.
    #[test]
    fn zooming_in_forever_stops_at_the_surface() {
        let mut orbit = Orbit::default();
        for _ in 0..1000 {
            orbit.zoom(1.0);
        }
        assert_eq!(orbit.altitude(), MIN_ALTITUDE_M);

        for _ in 0..1000 {
            orbit.zoom(-1.0);
        }
        assert_eq!(orbit.altitude(), MAX_ALTITUDE_M);
    }

    /// Zooming in and out by the same number of notches returns to the start.
    ///
    /// This is the statement "the scale is geometric": with added metres it
    /// would be just as true, but an altitude of 10 m would go negative on the
    /// very first notch of a 1e6 m step.
    #[test]
    fn zoom_is_geometric_and_reversible() {
        let mut orbit = Orbit::default();
        let start = orbit.altitude();

        for _ in 0..20 {
            orbit.zoom(1.0);
        }
        let closer = orbit.altitude();
        assert!(
            closer < start / 10.0,
            "twenty notches should have zoomed in"
        );

        for _ in 0..20 {
            orbit.zoom(-1.0);
        }
        assert!(
            (orbit.altitude() - start).abs() / start < 1e-9,
            "came back to {} instead of {start}",
            orbit.altitude()
        );
    }

    /// An altitude is measured above the body it was given, not above Earth.
    ///
    /// The reason this needs a test of its own is that the wrong answer here
    /// is a *working* camera, not a crash: asking for "10 km" over the Moon
    /// against Earth's radius puts the eye 4634 km up, and the picture from
    /// there is a perfectly ordinary picture of the Moon from far away. The
    /// tile census read exactly that as "six patches at every altitude" and
    /// nearly published it as a measurement.
    #[test]
    fn an_altitude_is_measured_above_the_body_it_was_given() {
        const MOON_RADIUS_M: f64 = 1_737_400.0;
        const ALTITUDE_M: f64 = 10.0e3;

        let moon = Orbit::around(MOON_RADIUS_M, ALTITUDE_M);
        assert_eq!(moon.distance(), MOON_RADIUS_M + ALTITUDE_M);

        let earth = Orbit::at_altitude(ALTITUDE_M);
        assert_eq!(earth.distance(), sphere::EARTH_RADIUS_M + ALTITUDE_M);

        // The two differ by the radii and by nothing else -- i.e. the altitude
        // itself carried over untouched.
        assert_eq!(
            earth.distance() - moon.distance(),
            sphere::EARTH_RADIUS_M - MOON_RADIUS_M
        );
    }

    /// Zooming does not forget which body the camera orbits.
    #[test]
    fn zooming_keeps_the_body_it_was_built_around() {
        const MOON_RADIUS_M: f64 = 1_737_400.0;

        let mut orbit = Orbit::around(MOON_RADIUS_M, 1.0e6);
        for _ in 0..1000 {
            orbit.zoom(1.0);
        }
        assert_eq!(orbit.distance(), MOON_RADIUS_M + MIN_ALTITUDE_M);
    }

    /// The default camera is the one the frame is measured with.
    #[test]
    fn the_default_view_is_the_one_the_shot_test_measures() {
        let from_orbit = Orbit::default().camera();
        let from_frame = crate::frame::default_camera();

        assert_eq!(from_orbit.position(), from_frame.position());
    }
}
