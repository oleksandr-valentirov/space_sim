//! The third-person camera: what the player looks at their own ship with
//! (stage V, step V4).
//!
//! Built like [`crate::orbit::Orbit`] and for the same reason: the state is
//! three numbers, and the position is **derived** from them rather than
//! accumulated. A camera that integrates offsets drifts off the sphere around
//! the target over time, and there is no moment where that becomes noticeable.
//!
//! There is no GPU here, no window and no `winit`: numbers in, a [`Camera`]
//! out.
//!
//! ## Two decisions the rest follows from
//!
//! **The camera takes position from the ship, not orientation.** Not a
//! simplification but the essence of a third-person view: a camera bound to the
//! ship's axes always shows it motionless -- roll, turning onto a heading and
//! swinging the nose are not visible in it at all, because the whole frame
//! turns with the ship. That is why the step's check is "a rotation about each
//! of the three axes changes the silhouette", and why it would only make sense
//! under this decision.
//!
//! **Distance is measured in ship extents, not metres.** The state carries
//! `ranges` -- how many times farther the camera is than the extent -- and it
//! turns into metres only when a [`Camera`] is built. Then the frame composes a
//! six-metre placeholder and a future real model of any size alike, and the
//! near limit ("do not climb inside the hull") is expressed by a number that
//! will not have to be revisited with every new ship.

use crate::camera::Camera;
use crate::scene::Ship;

/// The closest the wheel allows -- in ship extents.
///
/// One and a half extents, not one: the extent is the bounding-sphere radius,
/// so at one the camera would sit exactly on the hull. The half-extent margin
/// keeps the frame's near plane meaningful -- it is a tenth of the distance to
/// the hull (`Frame::near_for`), and at zero would hit the 0.1 m floor.
pub const MIN_RANGES: f64 = 1.5;

/// The farthest. Two hundred extents of a six-metre ship is a kilometre, that
/// is a vessel one and a half pixels across in a 720p frame. Beyond that the
/// third person has nothing to look at: that is a map, not a view.
pub const MAX_RANGES: f64 = 200.0;

/// Radians per pixel of mouse drag -- the same number as the orbit camera's:
/// two different rates in one game would feel to the player like a fault.
pub const RADIANS_PER_PIXEL: f64 = std::f64::consts::PI / 600.0;

/// The factor by which one wheel notch changes the distance. Geometric, as in
/// [`crate::orbit`]: from one and a half extents to two hundred is two and a
/// half orders of magnitude.
const ZOOM_PER_NOTCH: f64 = 1.25;

/// Where the pitch must not reach: exactly at the pole the view direction
/// coincides with the "up" reference, their cross product is zero, and the
/// camera basis becomes NaN. A user drags the camera to the pole in a
/// second.
const PITCH_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - 1.0e-3;

pub struct Chase {
    /// Azimuth about the "up" reference.
    yaw: f64,
    /// Elevation above the plane perpendicular to "up".
    pitch: f64,
    /// Distance to the ship in the ship's own extents.
    ranges: f64,
}

impl Default for Chase {
    /// From the side and slightly above -- the same angle the V2 demo
    /// animation starts from, and not by accident: from the nose the ship reads
    /// worst of all, and the third person should open on a recognisable
    /// silhouette.
    fn default() -> Self {
        Chase {
            yaw: std::f64::consts::FRAC_PI_2,
            pitch: 0.25,
            ranges: 4.5,
        }
    }
}

impl Chase {
    /// The same angle, but from a different distance -- in ship extents.
    pub fn at_ranges(ranges: f64) -> Chase {
        Chase {
            ranges: ranges.clamp(MIN_RANGES, MAX_RANGES),
            ..Chase::default()
        }
    }

    /// How far the camera is, in ship extents.
    pub fn ranges(&self) -> f64 {
        self.ranges
    }

    /// A mouse drag of `dx`, `dy` pixels.
    pub fn drag(&mut self, dx: f64, dy: f64) {
        self.yaw += dx * RADIANS_PER_PIXEL;
        self.pitch = (self.pitch + dy * RADIANS_PER_PIXEL).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// `notches` wheel clicks: positive zooms in.
    pub fn zoom(&mut self, notches: f64) {
        let factor = ZOOM_PER_NOTCH.powf(-notches);
        self.ranges = (self.ranges * factor).clamp(MIN_RANGES, MAX_RANGES);
    }

    /// A camera looking at `ship` from the current angles and distance.
    ///
    /// `up` is the "up" reference, and it **comes from the caller** rather than
    /// being derived here. Near a planet it is the local vertical, in an
    /// interplanetary transfer anything constant; `Scene` does not say which
    /// body the vessel is near, and guessing it from the nearest body would
    /// create a second truth about something the caller already knows exactly.
    ///
    /// The ship's orientation is deliberately not read -- see the module
    /// comment.
    pub fn camera(&self, ship: &Ship, up: [f64; 3]) -> Camera {
        let (east, north) = basis(up);
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let distance = self.ranges * ship.extent_m;

        let mut position = [0.0; 3];
        for k in 0..3 {
            let direction =
                cos_pitch * (cos_yaw * east[k] + sin_yaw * north[k]) + sin_pitch * up[k];
            position[k] = ship.centre[k] + distance * direction;
        }

        Camera::look_at(position, ship.centre, up)
    }
}

/// Two directions across `up` that complete it into a right-handed triple.
///
/// The reference vector is chosen by the smallest component of `up`: any fixed
/// choice degenerates where `up` coincides with it, and the smallest component
/// guarantees an angle of at least 54 degrees in the worst case.
fn basis(up: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let smallest = (0..3).fold(0, |best, k| {
        if up[k].abs() < up[best].abs() {
            k
        } else {
            best
        }
    });
    let mut reference = [0.0; 3];
    reference[smallest] = 1.0;

    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / n, v[1] / n, v[2] / n]
    };

    let east = unit(cross(reference, up));
    let north = cross(up, east);
    (east, north)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ship(extent_m: f64) -> Ship {
        Ship {
            centre: [4.1e6, -2.7e6, 3.3e6],
            orientation: [1.0, 0.0, 0.0, 0.0],
            height_m: 2.0 * extent_m,
            extent_m,
            colour: [0.7, 0.7, 0.75, 1.0],
            roughness: crate::ship::HULL_ROUGHNESS,
            metallic: crate::ship::HULL_METALLIC,
        }
    }

    /// A skewed "up" reference: no axis coincides with a world axis.
    fn up() -> [f64; 3] {
        let v = [0.37, -0.51, 0.77_f64];
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / n, v[1] / n, v[2] / n]
    }

    /// The ship stays dead ahead at any angles, and exactly at the distance
    /// the state promises.
    ///
    /// This checks the whole chain angles -> position -> camera basis at once: a
    /// sign error, a wrong multiplication order or swapped sin/cos would push
    /// the target aside. `relative64` is used rather than `relative`: at 5.9e6 m
    /// from the origin `f32` errs by metres, and the test would be measuring
    /// that instead of the camera.
    #[test]
    fn the_ship_stays_dead_ahead() {
        let ship = ship(3.0);
        for yaw_steps in 0..8 {
            for pitch_steps in -3..=3 {
                let mut chase = Chase::default();
                chase.drag(f64::from(yaw_steps) * 100.0, f64::from(pitch_steps) * 100.0);

                let centre = chase.camera(&ship, up()).relative64(ship.centre);
                assert!(
                    centre[0].abs() < 1.0e-6 && centre[1].abs() < 1.0e-6,
                    "the ship drifted aside: {centre:?}"
                );
                let distance = chase.ranges() * ship.extent_m;
                assert!(
                    (-centre[2] - distance).abs() < 1.0e-6,
                    "distance {} instead of {distance}",
                    -centre[2]
                );
            }
        }
    }

    /// A ship twice as large is framed the same way -- so the camera really
    /// does measure in extents rather than metres.
    ///
    /// The oracle is angular size: `atan(extent / distance)` must match. The
    /// 1e-9 tolerance is not "approximately" but the price of location: the ship
    /// stands 5.9e6 m from the origin, so subtracting the camera leaves about a
    /// nanometre, and the angle at thirteen metres of distance turns that into
    /// 1.5e-11 radians. Putting the ship at the origin would be simpler and
    /// would check less.
    #[test]
    fn a_bigger_ship_is_framed_the_same_way() {
        let small = ship(3.0);
        let big = ship(30.0);
        let chase = Chase::default();

        let angle = |s: &Ship| {
            let distance = chase.camera(s, up()).relative64(s.centre)[2].abs();
            (s.extent_m / distance).atan()
        };
        assert!(
            (angle(&small) - angle(&big)).abs() < 1.0e-9,
            "{} against {}",
            angle(&small),
            angle(&big)
        );
    }

    /// The wheel neither drives the camera into the hull nor lets it past the
    /// map's edge.
    ///
    /// A hundred notches each way is more than enough to reach both bounds, so
    /// what is checked is the clamping rather than the rate.
    #[test]
    fn the_wheel_stops_at_both_ends() {
        let mut chase = Chase::default();
        for _ in 0..100 {
            chase.zoom(1.0);
        }
        assert!(
            (chase.ranges() - MIN_RANGES).abs() < 1.0e-12,
            "zoomed in to {}",
            chase.ranges()
        );

        for _ in 0..200 {
            chase.zoom(-1.0);
        }
        assert!(
            (chase.ranges() - MAX_RANGES).abs() < 1.0e-12,
            "zoomed out to {}",
            chase.ranges()
        );
    }

    /// The pitch never reaches the pole, so the camera basis does not
    /// degenerate.
    ///
    /// What is checked is not the angle itself but what the bound exists for:
    /// the camera position stays finite and at its distance, rather than NaN.
    #[test]
    fn the_pitch_never_reaches_the_pole() {
        let ship = ship(3.0);
        let mut chase = Chase::default();
        for _ in 0..100 {
            chase.drag(0.0, 100.0);
        }
        let centre = chase.camera(&ship, up()).relative64(ship.centre);
        assert!(
            centre.iter().all(|v| v.is_finite()),
            "the basis degenerated: {centre:?}"
        );
        assert!(
            (-centre[2] - chase.ranges() * ship.extent_m).abs() < 1.0e-6,
            "the camera left the sphere: {centre:?}"
        );
    }

    /// Turning the ship does not move the camera at all -- the camera takes
    /// position, not orientation.
    ///
    /// The cheapest guard against orientation ever becoming "needed" here: with
    /// it the third-person view would stop showing rotation.
    #[test]
    fn turning_the_ship_does_not_move_the_camera() {
        let chase = Chase::default();
        let upright = ship(3.0);
        let mut rolled = ship(3.0);
        let half = std::f64::consts::FRAC_PI_4;
        rolled.orientation = [half.cos(), half.sin(), 0.0, 0.0];

        let a = chase.camera(&upright, up()).position();
        let b = chase.camera(&rolled, up()).position();
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1.0e-12, "{a:?} against {b:?}");
        }
    }
}
