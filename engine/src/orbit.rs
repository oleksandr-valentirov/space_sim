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
        Orbit {
            altitude: altitude.clamp(MIN_ALTITUDE_M, MAX_ALTITUDE_M),
            ..Orbit::default()
        }
    }

    pub fn altitude(&self) -> f64 {
        self.altitude
    }

    /// Distance from the planet's centre.
    pub fn distance(&self) -> f64 {
        sphere::EARTH_RADIUS_M + self.altitude
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

    pub fn camera(&self) -> Camera {
        let distance = self.distance();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();

        let position = [
            distance * cos_pitch * cos_yaw,
            distance * cos_pitch * sin_yaw,
            distance * sin_pitch,
        ];

        Camera::look_at(position, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
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

    /// The default camera is the one the frame is measured with.
    #[test]
    fn the_default_view_is_the_one_the_shot_test_measures() {
        let from_orbit = Orbit::default().camera();
        let from_frame = crate::frame::default_camera();

        assert_eq!(from_orbit.position(), from_frame.position());
    }
}
