//! Depth: reversed-Z with an infinite far plane (ROADMAP F3).
//!
//! PROJECT.md section 7 requires exactly this and forbids logarithmic depth
//! outright -- that one breaks early-Z and interpolation on large triangles.
//!
//! ## Why reversed-Z, in two lines of arithmetic
//!
//! A conventional projection puts the far away at `z_ndc = 1`, where float32 has
//! the worst absolute resolution, and additionally stretches the near through
//! `1/z`. Two troubles add up.
//!
//! Reversed-Z puts the far away at zero: `z_ndc = near / z`. Near zero float32
//! has an ULP proportional to the value itself, and `1/z` now compensates for
//! the distribution instead of spoiling it.
//!
//! ## The limit, and it is fundamental
//!
//! The resolvable gap at distance `z` follows from `dz_ndc = near*dz/z^2` and
//! `ulp(near/z) ~ (near/z)*2^-24`:
//!
//! ```text
//!     dz_min ~ z * 2^-24 ~ z * 6e-8
//! ```
//!
//! **`near` cancels.** The resolution is a constant fraction of the distance,
//! and no near plane improves it. That is not a property of the implementation
//! but a property of float32.
//!
//! A direct consequence: 1 m at 1e8 m is not resolvable **in principle** (the
//! limit there is ~8 m), while at 1e7 m it is (~0.8 m). That is why PROJECT.md
//! section 7 requires multi-pass rendering by ranges: one depth buffer will not
//! cover 1e11 m, and no setting changes that.
//!
//! Measured on the GPU in `--depth-probe`, matching the formula.

/// The depth buffer format. 32-bit floating point -- exactly what makes
/// reversed-Z meaningful: a 24-bit integer has no non-uniform ULP, and the whole
/// gain rests on that.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// How to compare depth. Reversed-Z: farther is smaller, so greater passes.
pub const COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::Greater;

/// What to clear with. Zero is "infinitely far".
pub const CLEAR: f32 = 0.0;

/// A 4x4 matrix, column-major, as the shader expects it.
pub type Matrix = [[f32; 4]; 4];

/// The resolution limit at distance `distance`, metres -- an **upper estimate**.
///
/// `f32::EPSILON` (2^-23) is used, that is the worst ULP within a binary
/// interval. The actual ULP wanders between 2^-24 and 2^-23 depending on the
/// mantissa, so the real limit can be up to 1.4 times better -- measured: at
/// 1e7 m the estimate gives 1.19 m, while the GPU still resolves a 1 m gap.
///
/// The upper estimate is deliberate: better to assume the worse than to expect
/// from depth something it will not do on a different mantissa.
pub fn resolvable_gap(distance: f64) -> f64 {
    distance * f64::from(f32::EPSILON)
}

/// Reversed-Z with an infinite far plane.
///
/// `z_clip = near`, `w_clip = -z_view`, that is `z_ndc = near / (-z_view)`: 1 at
/// the near plane, 0 at infinity.
pub fn reversed_infinite(fov_y: f64, aspect: f64, near: f64) -> Matrix {
    let f = 1.0 / (fov_y / 2.0).tan();

    [
        [(f / aspect) as f32, 0.0, 0.0, 0.0],
        [0.0, f as f32, 0.0, 0.0],
        [0.0, 0.0, 0.0, -1.0],
        [0.0, 0.0, near as f32, 0.0],
    ]
}

/// Reversed-Z with a **finite** far plane -- one range out of four
/// (ROADMAP-PLANETS.md, R4b).
///
/// `z_ndc = near*far/((far - near)*z) - near/(far - near)`: 1 at the near plane,
/// exactly 0 at the far one. As `far -> infinity` it degenerates into
/// [`reversed_infinite`], and that is no coincidence -- the same matrix with
/// `A = 0`.
///
/// ## What it buys, and what it does not -- measured
///
/// The resolution inside a range follows in closed form:
///
/// ```text
///     dz ~ z * 2^-24 * (1 - z/far)
/// ```
///
/// So near the range's **far** plane the resolution tends to zero (perfect),
/// while near the **near** one it tends to `z*2^-24`, that is exactly what a
/// single infinite projection gives. A consequence worth reading in full:
/// **splitting into ranges does not improve the worst case at a given
/// distance.** What it does buy is different and also valuable: two bodies in
/// different ranges do not compete for depth bits **at all**, and the order of
/// passes orders them. Verified from both sides in `tests/depth.rs`.
pub fn reversed_finite(fov_y: f64, aspect: f64, near: f64, far: f64) -> Matrix {
    assert!(far > near, "range {near}..{far} is empty or inverted");
    let f = 1.0 / (fov_y / 2.0).tan();
    let span = far - near;

    [
        [(f / aspect) as f32, 0.0, 0.0, 0.0],
        [0.0, f as f32, 0.0, 0.0],
        [0.0, 0.0, (near / span) as f32, -1.0],
        [0.0, 0.0, (near * far / span) as f32, 0.0],
    ]
}

/// A conventional projection with a finite far plane.
///
/// Not used by the engine. It exists for F3: without it "reversed-Z does not
/// flicker" is a claim without a comparison, and a check that has never failed
/// is worth nothing.
pub fn conventional(fov_y: f64, aspect: f64, near: f64, far: f64) -> Matrix {
    let f = 1.0 / (fov_y / 2.0).tan();

    [
        [(f / aspect) as f32, 0.0, 0.0, 0.0],
        [0.0, f as f32, 0.0, 0.0],
        [0.0, 0.0, (far / (near - far)) as f32, -1.0],
        [0.0, 0.0, (far * near / (near - far)) as f32, 0.0],
    ]
}

/// Depth settings for the conventional projection -- a mirror of the constants
/// above.
pub const CONVENTIONAL_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::Less;
pub const CONVENTIONAL_CLEAR: f32 = 1.0;

/// The depth of a point on the view axis at `distance` metres -- the same two
/// steps as in the shader (matrix multiply, perspective divide), in strict f32.
///
/// **This is deliberately a canonical computation, not a model of a particular
/// rasteriser.** Rust does not fuse multiply and add into an FMA (CLAUDE.md,
/// invariant 2), so the result is the same on every platform and equals what the
/// shader says literally. The hardware, as it turned out, carries more precision
/// than f32 through interpolation and division in places: in the F3 table the
/// conventional projection wins a frame at 1e4 m, although in strict f32 both
/// surfaces give the same bit there.
///
/// Hence the division of labour between this function and `depth_probe`:
///
/// - "depth **cannot** tell these apart" -- here. That is a property of the
///   format and the projection, and it is proved by matching bits. On the GPU
///   the same claim is unprovable: when both surfaces give the same value, the
///   winner is decided by tie-breaking and the driver's extra precision rather
///   than by depth -- which is exactly where the F3 test diverged between
///   llvmpipe, RADV and Metal.
/// - "depth **can** tell these apart" -- on the GPU. The difference really is
///   there, and the driver's extra precision does not eat it but only confirms
///   it.
pub fn ndc(projection: Matrix, distance: f64) -> f32 {
    let z_view = (-distance) as f32;

    let z_clip = projection[2][2] * z_view + projection[3][2];
    let w_clip = projection[2][3] * z_view + projection[3][3];

    z_clip / w_clip
}

/// The product of two matrices in the same layout as [`Matrix`] --
/// **column-major** (ROADMAP-PLANETS.md, R1d).
///
/// Needed by exactly one place: a patch brings vertices in world axes, so the
/// shader needs the projection multiplied by the view rotation. Doing that
/// multiplication in the shader per vertex would mean paying for it millions of
/// times per frame instead of once per frame here.
///
/// The layout is named out loud, because mixing it up silently is the easiest
/// thing to do: both matrices are 4x4 of `f32`, and a transposed product draws a
/// picture just as well, only not the right one.
pub fn multiply(a: Matrix, b: Matrix) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, value) in out_col.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same numbers `engine/tests/depth.rs` takes on the GPU.
    const NEAR: f64 = 0.1;
    const FOV_Y: f64 = std::f64::consts::PI / 3.0;

    /// The far plane is generous -- 10x the distance, as in
    /// `depth_probe::measure`.
    fn pair(reversed: bool, distance: f64, gap: f64) -> (f32, f32) {
        let projection = if reversed {
            reversed_infinite(FOV_Y, 1.0, NEAR)
        } else {
            conventional(FOV_Y, 1.0, NEAR, distance * 10.0)
        };

        (ndc(projection, distance), ndc(projection, distance - gap))
    }

    /// A conventional projection at 1e7 m cannot resolve even 100 m -- and this
    /// is not "almost".
    ///
    /// Both surfaces land exactly on the far plane: `z_ndc = 1.0` bitwise, and
    /// not because the case sits on a rounding boundary. `1 - near/z` at 1e7 m is
    /// long past the mantissa, so the same bit comes out for any gap up to
    /// several thousand metres; for `near = 0.1` that starts above ~1.7e6 m. So
    /// "take a gap with margin" does not help here in principle -- which is why
    /// the claim lives in arithmetic rather than next to its neighbours on the
    /// GPU.
    #[test]
    fn a_conventional_projection_collapses_a_hundred_metres_at_ten_million() {
        let (far, near) = pair(false, 1e7, 100.0);

        assert_eq!(
            far.to_bits(),
            near.to_bits(),
            "the conventional projection suddenly told these two surfaces \
             apart ({far} against {near}) -- then the F3 comparison proves \
             something other than claimed, and the cause must be found"
        );
        assert_eq!(
            far.to_bits(),
            1.0_f32.to_bits(),
            "both should have landed exactly on the far plane, and it came out \
             {far} -- then this is a different case from the one described above"
        );
    }

    /// A mirror of the previous one: without it "the conventional cannot" is
    /// not a comparison.
    #[test]
    fn reversed_z_keeps_the_same_pair_apart() {
        let (far, near) = pair(true, 1e7, 100.0);

        assert_ne!(
            far.to_bits(),
            near.to_bits(),
            "reversed-Z merged those same two surfaces into one bit ({far}) -- \
             then the whole reason to take it over the conventional one breaks"
        );
        assert!(
            near > far,
            "reversed-Z: closer is GREATER (hence COMPARE = Greater), and it \
             came out {near} against {far}"
        );
    }

    /// A limit rather than a bug: the same claim PROJECT.md section 7 rests
    /// on.
    #[test]
    fn a_metre_at_a_hundred_million_collapses_even_in_reversed_z() {
        assert!(
            resolvable_gap(1e8) > 1.0,
            "the estimate says a metre at 1e8 m should be resolvable -- then \
             the arithmetic in resolvable_gap has parted with reality"
        );

        let (far, near) = pair(true, 1e8, 1.0);

        assert_eq!(
            far.to_bits(),
            near.to_bits(),
            "a metre at 1e8 m is suddenly resolvable ({far} against {near}). \
             Good news, but it contradicts the calculation -- check the depth \
             format"
        );
    }

    /// How many ULP lie between two positive f32s. For the same sign and
    /// exponent the bits are monotonic, so a difference of bits is a distance in
    /// ULP.
    fn ulps_between(a: f32, b: f32) -> u32 {
        assert!(
            a > 0.0 && b > 0.0,
            "the ULP count here is for positives only"
        );
        a.to_bits().abs_diff(b.to_bits())
    }

    /// The smallest resolvable gap at distance `z` -- by binary search over
    /// bits.
    fn smallest_gap(projection: Matrix, z: f64) -> f64 {
        let (mut lo, mut hi) = (0.0f64, z);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if ndc(projection, z).to_bits() != ndc(projection, z - mid).to_bits() {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    }

    /// **A finite range is no sharper than an infinite one. Anywhere.**
    ///
    /// This is the main number of R4b, and it contradicts what was expected from
    /// splitting into ranges. The derivation promised `dz ~ z*2^-24*(1 - z/far)`,
    /// that is perfect resolution near the far plane; `f32` arithmetic does not
    /// deliver it, and the reason is simple -- `z_clip = A*z_view + B` near the
    /// far plane **cancels catastrophically**: two numbers of order `near` give
    /// zero, and the precision of the difference stays of order `near*2^-24`.
    /// Dividing by the slope `A = near/span` gives back the same `far*2^-24`.
    ///
    /// Measured (gap in metres, the same binary search over bits):
    ///
    /// ```text
    ///     z          infinite    z near far    z near near    z*6e-8
    ///     1e5 m        0.0039        0.0039         0.0039     0.0060
    ///     1e6 m        0.0313        0.0313         0.0312     0.0600
    ///     1e7 m        0.5000        0.5000         0.5000     0.6000
    ///     1e8 m        4.0000        4.0000         4.0000     6.0000
    ///     4e8 m       16.0000       16.0000        16.0000    24.0000
    /// ```
    ///
    /// Three columns agree to the bit. So **ranges do not buy resolution** --
    /// and the same goes for the boundary between them: the clipping plane
    /// stands on the same arithmetic, so two surfaces closer than `z*6e-8`
    /// cannot be separated by it either.
    ///
    /// What then justifies four passes is stated in `frame::Frame::plan`: not
    /// depth precision but scaled space, that is the right to draw the far away
    /// at an **invented** distance.
    #[test]
    fn a_finite_range_is_no_sharper_than_an_infinite_one() {
        for z in [1.0e5, 1.0e6, 1.0e7, 1.0e8, 4.0e8] {
            let infinite = smallest_gap(reversed_infinite(FOV_Y, 1.0, NEAR), z);
            let at_far = smallest_gap(reversed_finite(FOV_Y, 1.0, z / 100.0, z), z * 0.999);
            let at_near = smallest_gap(reversed_finite(FOV_Y, 1.0, z, z * 100.0), z * 1.001);

            println!(
                "  {z:9.1e}: infinite {infinite:.4} m, near far {at_far:.4} m, \
                 near near {at_near:.4} m, estimate {:.4} m",
                z * f64::from(f32::EPSILON)
            );

            // Equality rather than "approximately": three routes hit the same
            // mantissa. A tolerance here would hide the very thing measured.
            assert_eq!(
                infinite, at_far,
                "at {z:.1e} m the far edge of the range came out different -- \
                 then the R4b conclusion must be rewritten, not patched here"
            );
            // Near the near plane a difference of one search step is
            // acceptable: the distance itself is offset by 0.1%.
            assert!(
                (at_near - infinite).abs() <= infinite * 0.01,
                "at {z:.1e} m the near edge gave {at_near:.4} m against {infinite:.4}"
            );
            assert!(
                infinite <= z * f64::from(f32::EPSILON),
                "the measured gap {infinite:.4} m exceeds the upper estimate"
            );
        }
    }

    /// **A guard test for `engine/tests/depth.rs`, not a standalone claim about
    /// depth.**
    ///
    /// `reversed_z_resolves_a_metre_at_ten_million` stands on a difference of
    /// exactly 1 ULP -- that is the edge F3 measured, and there is deliberately
    /// no margin there. While the difference exists the claim is legitimate; if
    /// the arithmetic ever shifts by one bit, that test goes red **on the GPU**,
    /// that is where the cause is hardest to see: a black frame and someone
    /// else's rasteriser look the same.
    ///
    /// So the number is pinned here, in strict f32, where it lives. Reading
    /// order when red: this test first, and only if it is green -- the engine,
    /// the driver and the hardware.
    ///
    /// **A measured warning the ROADMAP did not have: the margin depends not
    /// only on the distance but on the mantissa of `near`.** The same metre at
    /// the same 1e7 m gives:
    ///
    /// ```text
    ///     near = 0.1   -> 1 ULP     near = 0.15  -> 0 ULP
    ///     near = 0.2   -> 1 ULP     near = 1.0   -> 1 ULP
    /// ```
    ///
    /// So powers of two times 0.1 give the same bits (`near` really does cancel
    /// -- the module says as much), while 0.15 **collapses** the cell. The
    /// practical consequence: if the camera's `near` is ever changed to
    /// something that is not a power of two, the GPU test may stay green through
    /// the rasteriser's tie-breaking while proving nothing. This test says so
    /// directly and at once.
    ///
    /// What it does **not** catch, and this was verified by mutation: moving
    /// `ndc` into f64 with a final narrowing to f32 gives the same bits. Not a
    /// flaw but the limit of its purpose: it guards one number, not the module's
    /// arithmetic.
    #[test]
    fn a_metre_at_ten_million_is_exactly_one_ulp() {
        let (far, near) = pair(true, 1e7, 1.0);

        assert!(
            near > far,
            "reversed-Z: closer is GREATER, and it came out {near} against {far}"
        );
        assert_eq!(
            ulps_between(near, far),
            1,
            "the margin of this cell has changed: between {near} and {far} \
             there are now {} ULP, not 1. If zero -- \
             reversed_z_resolves_a_metre_at_ten_million in engine/tests/depth.rs \
             no longer proves anything and passes by accident; if more -- the \
             depth arithmetic has changed, and the number in PROJECT.md \
             section 7 must be re-measured, not patched here",
            ulps_between(near, far)
        );
    }
}
