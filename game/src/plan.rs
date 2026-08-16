//! The manoeuvre plan (ROADMAP J3, PROJECT.md §8).
//!
//! A manoeuvre is `(time, dv in a frame)`, a plan is a list of manoeuvres, and
//! recomputation is cascading. That is exactly what §8 calls the flight
//! planner, in the minimal form that can already be executed: an **impulsive**
//! dv.
//!
//! ## Why impulsive rather than with a burn duration
//!
//! A finite burn needs thrust in C's force model, which is not there and will
//! not be until M3.5. An impulse is executed with what already exists:
//! propagate to ignition, add dv to the velocity, continue. The plan's shape is
//! ready for duration -- a field will appear, not a different mechanism.
//!
//! ## The determinism boundary runs here
//!
//! PROJECT.md §4: "Simulating a given plan must match bit for bit; how the
//! player came up with that plan need not." A plan is **data**: two numbers
//! and a frame. Lambert, porkchop and differential correction may give
//! slightly different numbers on different machines, and that is allowed; what
//! comes out of the resulting plan may not.
//!
//! Converting dv from a frame into inertial coordinates happens **inside** the
//! determinism boundary: only `+ - * /` and `sqrt` there, the same operations
//! CLAUDE.md allows in the integration loop (invariant 3). There is no
//! trigonometry here and there must be none.

use core_rs::State;

/// What the dv components are expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Barycentric inertial -- the frame everything else is computed in.
    /// That is how solvers give dv: Lambert returns velocity vectors, not "so
    /// much forward".
    Inertial,

    /// Along velocity / normal to the plane / outwards, relative to `body`.
    ///
    /// This is how the player thinks: "a hundred metres per second forward".
    /// Necessarily relative to a body: in barycentric coordinates a vessel's
    /// velocity near Earth is mostly Earth's own velocity around the Sun, and
    /// "forward" would mean along Earth's orbit.
    Vnb { body: i32 },
}

/// One manoeuvre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Manoeuvre {
    /// Ignition time, seconds from the asset epoch. Absolute.
    ///
    /// Not "at the third periapsis": event anchors will exist too, but they
    /// resolve to absolute time on recomputation and go into the save in that
    /// form. Otherwise a save would mean different things depending on when it
    /// was read.
    pub t: f64,
    /// Components in the [`Frame`], m/s.
    pub dv: [f64; 3],
    pub frame: Frame,
}

impl Manoeuvre {
    /// dv in barycentric inertial coordinates.
    ///
    /// `body` is the reference body's state at the manoeuvre's instant; for
    /// [`Frame::Inertial`] it is not needed and not read.
    pub fn dv_inertial(&self, vessel: &State, body: Option<&State>) -> [f64; 3] {
        match self.frame {
            Frame::Inertial => self.dv,
            Frame::Vnb { .. } => {
                let Some(body) = body else {
                    // The caller must supply a body; without one there is no
                    // basis. Silently taking the inertial frame would mean
                    // executing a different manoeuvre and not saying so.
                    return [0.0, 0.0, 0.0];
                };

                let r = [
                    vessel.r.x - body.r.x,
                    vessel.r.y - body.r.y,
                    vessel.r.z - body.r.z,
                ];
                let v = [
                    vessel.v.x - body.v.x,
                    vessel.v.y - body.v.y,
                    vessel.v.z - body.v.z,
                ];

                let prograde = normalize(v);
                let normal = normalize(cross(r, v));
                // Completes the right-handed triple: outwards from the body
                // in the orbital plane.
                let outward = cross(prograde, normal);

                [
                    self.dv[0] * prograde[0] + self.dv[1] * normal[0] + self.dv[2] * outward[0],
                    self.dv[0] * prograde[1] + self.dv[1] * normal[1] + self.dv[2] * outward[1],
                    self.dv[0] * prograde[2] + self.dv[1] * normal[2] + self.dv[2] * outward[2],
                ]
            }
        }
    }

    /// The body the frame is relative to, if any.
    pub fn frame_body(&self) -> Option<i32> {
        match self.frame {
            Frame::Inertial => None,
            Frame::Vnb { body } => Some(body),
        }
    }
}

/// A list of manoeuvres, ordered in time.
///
/// The order is an invariant of the type rather than a convention: the segment
/// loop takes the next manoeuvre by index and stops at its time, so an
/// unordered plan would mean a run into the past.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    manoeuvres: Vec<Manoeuvre>,
}

impl Plan {
    pub fn new() -> Plan {
        Plan::default()
    }

    pub fn manoeuvres(&self) -> &[Manoeuvre] {
        &self.manoeuvres
    }

    pub fn is_empty(&self) -> bool {
        self.manoeuvres.is_empty()
    }

    pub fn len(&self) -> usize {
        self.manoeuvres.len()
    }

    pub fn get(&self, index: usize) -> Option<&Manoeuvre> {
        self.manoeuvres.get(index)
    }

    /// Adds a manoeuvre, preserving time order.
    pub fn insert(&mut self, m: Manoeuvre) {
        let at = self.manoeuvres.partition_point(|other| other.t <= m.t);
        self.manoeuvres.insert(at, m);
    }

    /// The earliest instant at which two plans differ.
    ///
    /// This is the point to recompute from, and it comes from nowhere else:
    /// comparing trajectories would be both more expensive and later -- they
    /// diverge after a manoeuvre rather than at it.
    pub fn diverges_from(&self, other: &Plan) -> Option<f64> {
        let mine = &self.manoeuvres;
        let theirs = &other.manoeuvres;

        for (a, b) in mine.iter().zip(theirs.iter()) {
            if a != b {
                // The earlier of the two: a manoeuvre could both disappear and
                // appear earlier.
                return Some(a.t.min(b.t));
            }
        }

        // Equal as far as they overlap; the remainder is a tail appearing or
        // disappearing.
        match mine.len().cmp(&theirs.len()) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Less => Some(theirs[mine.len()].t),
            std::cmp::Ordering::Greater => Some(mine[theirs.len()].t),
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_rs::Vec3d;

    fn state(r: [f64; 3], v: [f64; 3]) -> State {
        State {
            r: Vec3d {
                x: r[0],
                y: r[1],
                z: r[2],
            },
            v: Vec3d {
                x: v[0],
                y: v[1],
                z: v[2],
            },
            t: 0.0,
        }
    }

    /// The VNB basis is orthonormal and oriented as promised.
    ///
    /// A circular orbit in the xy plane: "forward" must land on +y, "normal" on
    /// +z, "outwards" on +x. An error in the cross product's order would give a
    /// mirrored basis, and a "forward" manoeuvre would brake.
    #[test]
    fn the_vnb_basis_points_where_it_says() {
        let vessel = state([1.0e7, 0.0, 0.0], [0.0, 3.0e3, 0.0]);
        let body = state([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);

        let along = Manoeuvre {
            t: 0.0,
            dv: [10.0, 0.0, 0.0],
            frame: Frame::Vnb { body: 3 },
        };
        assert_eq!(along.dv_inertial(&vessel, Some(&body)), [0.0, 10.0, 0.0]);

        let normal = Manoeuvre {
            dv: [0.0, 10.0, 0.0],
            ..along
        };
        assert_eq!(normal.dv_inertial(&vessel, Some(&body)), [0.0, 0.0, 10.0]);

        let outward = Manoeuvre {
            dv: [0.0, 0.0, 10.0],
            ..along
        };
        assert_eq!(outward.dv_inertial(&vessel, Some(&body)), [10.0, 0.0, 0.0]);
    }

    /// The frame is computed relative to the body, not the barycentre.
    ///
    /// A body moving faster than the vessel is precisely the case where the
    /// difference is not cosmetic: in barycentric coordinates "forward" would
    /// point along the body's motion.
    #[test]
    fn the_frame_follows_the_body_not_the_barycentre() {
        let body = state([0.0, 0.0, 0.0], [3.0e4, 0.0, 0.0]);
        let vessel = state([1.0e7, 0.0, 0.0], [3.0e4, 3.0e3, 0.0]);

        let along = Manoeuvre {
            t: 0.0,
            dv: [10.0, 0.0, 0.0],
            frame: Frame::Vnb { body: 3 },
        };
        assert_eq!(along.dv_inertial(&vessel, Some(&body)), [0.0, 10.0, 0.0]);
    }

    /// The plans' divergence is found at the earliest change, from either
    /// side.
    #[test]
    fn divergence_finds_the_earliest_change() {
        let m = |t: f64, dv: f64| Manoeuvre {
            t,
            dv: [dv, 0.0, 0.0],
            frame: Frame::Inertial,
        };

        let mut a = Plan::new();
        a.insert(m(100.0, 1.0));
        a.insert(m(200.0, 2.0));

        assert_eq!(a.diverges_from(&a.clone()), None);

        // The second manoeuvre changed.
        let mut b = a.clone();
        b.manoeuvres[1] = m(200.0, 5.0);
        assert_eq!(a.diverges_from(&b), Some(200.0));

        // Shifted in time -- the earlier of the two instants, because the
        // recomputation must start where the plans already differ.
        let mut c = a.clone();
        c.manoeuvres[1] = m(150.0, 2.0);
        assert_eq!(a.diverges_from(&c), Some(150.0));

        // A tail appended.
        let mut d = a.clone();
        d.insert(m(300.0, 3.0));
        assert_eq!(a.diverges_from(&d), Some(300.0));
        assert_eq!(d.diverges_from(&a), Some(300.0));
    }

    /// Insertion keeps time order however it is called.
    #[test]
    fn insertion_keeps_the_order() {
        let m = |t: f64| Manoeuvre {
            t,
            dv: [0.0; 3],
            frame: Frame::Inertial,
        };

        let mut plan = Plan::new();
        for t in [300.0, 100.0, 200.0, 50.0] {
            plan.insert(m(t));
        }

        let times: Vec<f64> = plan.manoeuvres().iter().map(|m| m.t).collect();
        assert_eq!(times, vec![50.0, 100.0, 200.0, 300.0]);
    }
}
