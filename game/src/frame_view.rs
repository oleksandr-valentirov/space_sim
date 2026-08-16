//! The frame the scene is built in: inertial or rotating (ROADMAP-UI.md,
//! U6a).
//!
//! ## Why the transform is here and not in the vertex shader
//!
//! PROJECT.md §7 long promised the opposite -- "the key trick: we do the
//! transform into the rotating frame in the vertex shader". U6a1 measured
//! that, and §7 changed: the shader path needs world coordinates in the vertex
//! (`f32`, up to 4e8 m), which gives **132 m** of worst-case error -- 17 pixels
//! with a 10 km wide frame, and as noise rather than a constant offset. The
//! CPU transform in `f64` costs 2.69 -> 10.56 ns per point, i.e. adds no pass:
//! `Lines::upload` already walks every point every frame.
//!
//! ## What exactly is transformed
//!
//! **The whole scene, not the polyline alone.** In the synodic frame Earth and
//! the Moon are stationary -- that is, their centres go through the same
//! transform as the trajectory's points, or the sphere would hang apart from
//! the trajectory around it.
//!
//! Every trajectory point takes the basis of **its own instant** (which is why
//! the sample carries Earth's and the Moon's positions), while bodies and
//! markers take the "now" basis. The scale is one for the whole frame: synodic
//! units are dimensionless and are multiplied by the current Earth-Moon
//! distance. Because of that every sample's Moon lands exactly where the Moon
//! is now -- and that is what makes the Lagrange points stationary.

/// How many metres one synodic distance unit is on screen.
///
/// **A constant, not the current Earth-Moon distance** -- and that is not a
/// detail. Synodic coordinates are dimensionless: in them the Moon always sits
/// at exactly `1 - mu`. Multiplying them by the current distance would put
/// back into the picture exactly what the frame just removed: `L` wanders
/// between 3.63 and 4.06e8 m, so the Moon would move again -- slower than
/// inertially, but move. Measured at U6a3: over three days that shifts the
/// Moon's disc by its own diameter.
///
/// The value is the mean Earth-Moon distance. Any other would merely change
/// the picture's scale: this is a choice of appearance, not of physics.
pub const SYNODIC_SCALE_M: f64 = 3.844e8;

/// Which frame the game asks the scene to be drawn in.
///
/// This is **view** state, not world state: no number in the snapshot changes
/// because of it (rule 1 of stage U).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewFrame {
    /// Geocentric inertial -- what the frame drew before U6a.
    #[default]
    Inertial,
    /// Rotating (synodic) Earth-Moon: both bodies stand still.
    Rotating,
}

/// The synodic basis of one instant.
///
/// Orthonormal by construction: `x` along Earth-Moon, `z` the normal of the
/// instantaneous plane, `y = z x x`.
#[derive(Clone, Copy, Debug)]
pub struct Synodic {
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    /// The Earth-Moon distance at that instant, metres -- coordinates are
    /// divided by it.
    length: f64,
    /// How many metres one synodic distance unit is in this frame: the same
    /// distance, but the **current** one. One for the whole frame, hence the
    /// stationary Moon.
    scale: f64,
    /// The Moon's mass fraction, `mu`: the barycentre sits `mu*L` from
    /// Earth.
    mass_ratio: f64,
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = length(v);
    // A zero length is not "almost zero" but a missing basis: bodies at one
    // point, or a motionless Moon. A silent division would give NaN at every
    // vertex of the frame.
    if n > 0.0 {
        Some([v[0] / n, v[1] / n, v[2] / n])
    } else {
        None
    }
}

impl Synodic {
    /// A basis from the Earth-to-Moon vector and the plane's normal.
    ///
    /// `normal` need be neither unit nor strictly perpendicular to `d`: it is
    /// orthogonalised right here. The reason is practical -- the normal comes
    /// from a central difference over samples, and demanding precision from it
    /// would push that work onto every caller.
    pub fn new(d: [f64; 3], normal: [f64; 3], scale: f64, mass_ratio: f64) -> Option<Synodic> {
        let length = length(d);
        let x = unit(d)?;
        let z = unit(cross(x, cross(normal, x)))?;
        let y = cross(z, x);

        Some(Synodic {
            x,
            y,
            z,
            length,
            scale,
            mass_ratio,
        })
    }

    /// The same frame, but another instant's basis: its own Earth-Moon line
    /// and its own normal, with the current scale and `mu`.
    ///
    /// That is exactly how a sample's basis differs from the "now" basis: one
    /// scale for the whole frame, otherwise the trajectory would breathe along
    /// with the distance to the Moon.
    pub fn with_line(&self, d: [f64; 3], normal: [f64; 3]) -> Option<Synodic> {
        Synodic::new(d, normal, self.scale, self.mass_ratio)
    }

    /// A geocentric point to a synodic one, in metres of the current scale.
    pub fn apply(&self, geocentric: [f64; 3], d: [f64; 3]) -> [f64; 3] {
        // The origin is the pair's barycentre rather than Earth: that is where
        // CR3BP keeps its Lagrange points, and the zero-velocity curve (U6b)
        // will be computed from it too.
        let rel = [
            geocentric[0] - self.mass_ratio * d[0],
            geocentric[1] - self.mass_ratio * d[1],
            geocentric[2] - self.mass_ratio * d[2],
        ];
        let k = self.scale / self.length;
        [
            dot(rel, self.x) * k,
            dot(rel, self.y) * k,
            dot(rel, self.z) * k,
        ]
    }

    /// A world direction into this basis -- the rotation alone, without origin
    /// or scale.
    ///
    /// Separate from [`Synodic::apply`] because a direction is not a point:
    /// the shift to the barycentre and the division by the Earth-Moon distance
    /// would turn a unit vector into something other than a direction. The
    /// scene's light reads it (V5).
    pub fn direction(&self, v: [f64; 3]) -> [f64; 3] {
        [dot(v, self.x), dot(v, self.y), dot(v, self.z)]
    }

    /// The rotation from world into this basis, as a quaternion `[w, x, y, z]`.
    ///
    /// The bodies need it: their orientation arrives in the scene as a
    /// rotation from body frame into world, and in a rotating frame "world" is
    /// already different. For a smooth sphere that is invisible (R1e measured
    /// that rotating a sphere changes no pixel), but from R5 the bodies gain a
    /// surface -- and then an unrotated Earth becomes a bug to be hunted after
    /// the fact.
    pub fn rotation(&self) -> [f64; 4] {
        // The matrix's rows are the basis: `B^T * v = (v.x, v.y, v.z)`.
        let m = [self.x, self.y, self.z];
        quat_from_rows(m)
    }
}

/// A quaternion `[w, x, y, z]` from a rotation matrix given by rows.
///
/// Branching on the largest element rather than one formula: in the formula
/// through `w` the denominator tends to zero at a 180-degree rotation, and the
/// quaternion falls apart exactly where the matrix is perfectly healthy.
pub fn quat_from_rows(m: [[f64; 3]; 3]) -> [f64; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];

    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        return [
            0.25 * s,
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
        ];
    }
    if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        return [
            (m[2][1] - m[1][2]) / s,
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
        ];
    }
    if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        return [
            (m[0][2] - m[2][0]) / s,
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
        ];
    }
    let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
    [
        (m[1][0] - m[0][1]) / s,
        (m[0][2] + m[2][0]) / s,
        (m[1][2] + m[2][1]) / s,
        0.25 * s,
    ]
}

/// Quaternion product `[w, x, y, z]`: `b` first, then `a`.
pub fn compose(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
        let [w, x, y, z] = q;
        let m = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// The basis's quaternion does to a vector what the basis itself does.
    ///
    /// The oracle is not "similar numbers" but the equality of two independent
    /// paths: three dot products against a quaternion rotation. A divergence
    /// between them is exactly what would give a body rotated differently from
    /// the trajectory around it.
    #[test]
    fn the_quaternion_of_a_basis_turns_vectors_the_way_the_basis_does() {
        // The basis is deliberately skewed: an axis-aligned case would pass
        // even with the components swapped.
        let d = [3.4e8, -1.7e8, 0.9e8];
        let normal = [0.1, 0.3, 1.0];
        let s = Synodic::new(d, normal, 1.0, 0.0).expect("the basis exists");
        let q = s.rotation();

        for v in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [2.0e7, -3.0e7, 5.0e6],
        ] {
            let by_basis = [dot(v, s.x), dot(v, s.y), dot(v, s.z)];
            let by_quat = rotate(q, v);
            for k in 0..3 {
                assert!(
                    (by_basis[k] - by_quat[k]).abs() < 1e-6 * (1.0 + by_basis[k].abs()),
                    "component {k}: {:?} against {:?}",
                    by_basis,
                    by_quat
                );
            }
        }
    }

    /// In the synodic frame the Moon sits at `(1 - mu)` from the barycentre,
    /// on the x axis.
    ///
    /// The frame's definition written as a number: if the sign of `mu` or the
    /// order of multiplication by the scale is confused anywhere, the Moon
    /// leaves the axis.
    #[test]
    fn the_moon_sits_on_the_x_axis_at_one_minus_mu() {
        let d = [3.4e8, -1.7e8, 0.9e8];
        let l = length(d);
        let mu = 0.012_150_585_609_624_04;
        let s = Synodic::new(d, [0.1, 0.3, 1.0], l, mu).expect("the basis exists");

        let moon = s.apply(d, d);
        assert!(
            (moon[0] - (1.0 - mu) * l).abs() < 1.0,
            "the Moon is at {moon:?}"
        );
        assert!(
            moon[1].abs() < 1.0 && moon[2].abs() < 1.0,
            "the Moon is at {moon:?}"
        );

        let earth = s.apply([0.0, 0.0, 0.0], d);
        assert!((earth[0] + mu * l).abs() < 1.0, "Earth is at {earth:?}");
        assert!(
            earth[1].abs() < 1.0 && earth[2].abs() < 1.0,
            "Earth is at {earth:?}"
        );
    }

    /// The "now" scale makes the Moon stationary though the distance to it
    /// wanders.
    ///
    /// Two different `d` of different lengths, the same `scale` -- and the Moon
    /// is at one point in both cases. Without this the 3.63-4.06e8 m range
    /// would rock the picture by 10%.
    #[test]
    fn the_moon_stands_still_although_its_distance_does_not() {
        let mu = 0.012_150_585_609_624_04;
        let scale = 3.84e8;
        let near = [3.63e8, 0.0, 0.0];
        let far = [0.0, 4.06e8, 0.0];

        let a = Synodic::new(near, [0.0, 0.0, 1.0], scale, mu).expect("basis");
        let b = Synodic::new(far, [0.0, 0.0, 1.0], scale, mu).expect("basis");

        let moon_a = a.apply(near, near);
        let moon_b = b.apply(far, far);
        for k in 0..3 {
            assert!(
                (moon_a[k] - moon_b[k]).abs() < 1.0,
                "the Moon moved: {moon_a:?} against {moon_b:?}"
            );
        }
    }

    /// A degenerate basis never happens silently.
    #[test]
    fn a_basis_that_cannot_exist_says_so() {
        assert!(Synodic::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0, 0.0).is_none());
        // A normal along the line of bodies itself defines no plane.
        assert!(Synodic::new([1.0, 0.0, 0.0], [2.0, 0.0, 0.0], 1.0, 0.0).is_none());
    }
}
