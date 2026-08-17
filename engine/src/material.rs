//! The material rule: surface colour from slope and relief (ROADMAP, T4).
//!
//! The Moon's colour tileset carries six levels, that is a node ~2.7 km wide,
//! while the source (LROC WAC) is 100 m per pixel, from which the cooker takes a
//! point sample. Terrain, meanwhile, gets octaves grown by [`crate::detail`]
//! down to ~106 m. So up close the mountains are sharp while the colour over
//! them is a flat blob, and the gap between them grows with every step the
//! camera takes downward.
//!
//! That gap cannot be filled with a second texture: there simply is none. So
//! what R7c did for height is done here for colour -- by a **rule** rather than
//! by data: brightness is shifted by the terrain slope and by the procedural
//! relief standing on it.
//!
//! ## Why slope and relief rather than height above the sphere
//!
//! Height above the reference radius is **already** in the colour: seas lie
//! lower and are darker, continents higher and lighter, and the mosaic knows
//! that from measurements. A rule that added its own dependence on height would
//! paint a second answer to the same question over the data -- and on Earth (T7)
//! would visibly disagree with the bathymetry.
//!
//! Slope has no such duplicate: at the scale of a mosaic node it is not visible
//! at all, and the physics behind it is real -- on a steep slope regolith slides
//! away, exposing lighter immature material; that is why the walls of fresh
//! craters are bright.
//!
//! WARNING: **water is an exception, and it does not contradict the above**
//! (T7f). Height enters the rule by exactly one bit: "is the surface we see the
//! one the DEM describes". Below sea level it is not: what is in the frame there
//! is water, while the slope beneath it belongs to the sea floor. The strength
//! of the rule does not depend on height there either: it is either the same or
//! zero. That is a different quantity from "brightness grows with height", which
//! the paragraph above argues against.
//!
//! ## Earth does not get its own `SLOPE_REF`, and that is a decision (T7f)
//!
//! The slope distribution on `assets/earth.dem` is four times gentler than the
//! Moon's on the same measurement base: median 0.0058 against 0.035, ninetieth
//! percentile 0.0295 against 0.128. The temptation to recompute [`SLOPE_REF`]
//! for Earth is strong and wrong: on the Moon the rule exists because there is
//! no colour data there (the single-channel WAC blob), while Earth has the Blue
//! Marble mosaic in the **same** grid as the DEM. A threshold fitted to Earth's
//! distribution would give mountains the full +30% brightness on top of measured
//! albedo -- that is, paint over the data with a rule. With the constant at 0.15
//! the rule is quiet on Earth (1.06 at the ninetieth percentile of land), and
//! that is exactly what is wanted from it here.
//!
//! Ice gets no branch of its own either: in the mosaic it is already white. A
//! rule for it would be a second answer to a question the data has answered.
//!
//! ## Three rules inherited from the height detail
//!
//! **1. The arguments are quantities, not asset state.** `slope` arrives in
//! radians-per-unit, `detail_m` in metres, the normaliser comes from the
//! **body's radius** ([`crate::detail::base_m`]). The pyramid depth enters the
//! rule nowhere: recooking the asset with one more level has no right to repaint
//! a slope, just as it has no right to replay the mountains.
//!
//! **2. Computed in the vertex, and that is why there is no seam.** Both
//! arguments are already computed by `vertex_terrain` and both are **bitwise
//! identical** at a node shared by two patches -- `slope` by R7c, roughness as a
//! function of the normal from the geometry buffer. So colour continuity at a
//! level boundary is not proved but inherited.
//!
//! WARNING: the temptation to take the slope from `ddx/ddy` in the fragment is
//! strong -- the true geometric normal with all the relief is there, and for
//! free. But it is the normal of a **triangle**, that is a function of the patch
//! level: two neighbours of different levels would give two different colours on
//! the shared edge, and a seam would fall exactly where R2b removed it.
//!
//! **3. Flat ground stays flat ground.** Both terms are multiplied by the same
//! slope, so on a sea floor the rule returns exactly one, not "almost one": the
//! colour there stays what WAC measured.

/// How much brighter the steepest slope is than flat ground.
///
/// A look parameter, not a measured quantity: the real photometric contrast of
/// fresh regolith depends on the phase angle, which the frame does not have yet.
/// Change freely while the stage's checks are green.
pub const SLOPE_GAIN: f64 = 0.30;

/// The slope at which the slope highlight reaches full strength.
///
/// **Measured on a real asset rather than taken from physics.** The temptation
/// to put the angle of repose of regolith here (~0.3, that is 17 degrees) is
/// strong and wrong: that angle concerns the **local** slope, while `slope_at`
/// returns a slope measured at the node step of the tile the patch reads --
/// 5330 m at the Moon's deepest level, that is smoothed.
/// The distribution over `assets/moon.dem` (31,104 nodes of the deepest level):
///
/// | fraction | slope | angle |
/// |---|---|---|
/// | 50% | 0.035 | 2.0 deg |
/// | 90% | 0.128 | 7.3 deg |
/// | 99% | 0.221 | 12.5 deg |
/// | maximum | 0.410 | 22.3 deg |
///
/// So at 0.3 the rule would switch on over one thousandth of the body and do
/// nothing on the rest. 0.15 is roughly the ninetieth percentile: half the
/// surface gets a quarter of the range, crater walls saturate it, and a flat sea
/// floor stays flat.
///
/// WARNING: the number is tied to the **measurement base**, not to a body, and
/// remains an engine constant after T7: Earth gave a distribution four times
/// gentler, but the rule is supposed to be quiet there -- the argument is in the
/// module introduction.
pub const SLOPE_REF: f64 = 0.15;

/// How much brighter a procedural relief crest is than the hollow beside it.
///
/// A factor on [`crate::detail::Detail::roughness`] -- a number of order one in
/// which all octaves weigh the same. The spread of the roughness itself is +-0.5
/// in practice, so on a slope of [`SLOPE_REF`] that is about +-0.2 of
/// brightness, and exactly zero on flat ground.
///
/// WARNING: the first version of the rule took not the roughness but the
/// **detail height in metres**, divided by its own amplitude. Measured on a
/// frame: +-1% of brightness, that is less than one level of an eight-bit scale
/// -- the rule was in the frame and was not visible. The cause was not the
/// factor: in height the coarsest octave weighs 32 times more than the finest,
/// so the colour would carry a 3.4 km blob instead of detail.
pub const RELIEF_GAIN: f64 = 0.45;

/// Bounds on the factor.
///
/// Not cosmetic: albedo is never negative, and a factor above ~2 would push the
/// brightest parts of the mosaic past the end of the scale before the
/// tonemapper (which arrives in T5).
pub const MIN_TINT: f64 = 0.35;
/// The upper bound on the factor, the counterpart of [`MIN_TINT`].
pub const MAX_TINT: f64 = 1.80;

/// The albedo factor at a node.
///
/// * `slope` -- the terrain slope, [`crate::tiles::Terrain::slope_at`];
/// * `roughness` -- [`crate::detail::Detail::roughness`] at the same node.
///
/// Neither argument comes from the asset: the first is a quantity in metres per
/// metre, the second is noise by position on the body. Neither the pyramid depth
/// nor the tileset step has any way in here, and that is why recooking the asset
/// does not repaint slopes.
///
/// Both terms are multiplied by the slope -- that is what "flat ground stays
/// flat ground" means: colour noise appears exactly where there is relief to
/// justify it.
///
/// The third argument is `submerged`, that is "this node is lower than the
/// body's sea level" ([`crate::tiles::Terrain::sea_units`]). Under water the
/// rule returns a **bitwise one**, and the reason is not taste: it highlights a
/// slope, and under water what the frame shows is not the slope of the sea floor
/// but the surface of the sea. Measured on `assets/earth.dem`
/// (`--example slope_histogram assets/earth.dem`):
///
/// | | median | 90% | 99% | factor at 90% |
/// |---|---|---|---|---|
/// | water | 0.0071 | 0.0333 | 0.0996 | 1.067 |
/// | land | 0.0030 | 0.0201 | 0.0557 | 1.040 |
///
/// So the sea floor is **steeper** than the land -- mid-ocean ridges and
/// trenches -- and without this branch the rule would draw bathymetry over flat
/// water, and brighter than mountains on land at that. The same argument as in
/// the module introduction: do not give a second answer to a question the data
/// has already answered.
pub fn tint(slope: f64, roughness: f64, submerged: bool) -> f64 {
    if submerged {
        return 1.0;
    }
    let steep = (slope / SLOPE_REF).clamp(0.0, 1.0);
    (1.0 + steep * (SLOPE_GAIN + RELIEF_GAIN * roughness)).clamp(MIN_TINT, MAX_TINT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cubesphere::{Patch, FACES, SIDE};
    use crate::detail;
    use crate::tiles::{self, Terrain, HALO, STORED};

    const RADIUS: f64 = 1_737_400.0;

    /// Terrain with a constant slope -- linear in face fractions.
    fn ramp(levels: u32) -> Terrain {
        let mut grids = Vec::with_capacity(Terrain::count(levels));
        for level in 0..levels {
            let side = 1u32 << level;
            for _face in 0..FACES {
                for i in 0..side {
                    for j in 0..side {
                        let span = f64::from(SIDE as u32 * side);
                        let mut grid = Vec::with_capacity(STORED * STORED);
                        for a in 0..STORED {
                            for b in 0..STORED {
                                let a = a as isize - HALO as isize;
                                let b = b as isize - HALO as isize;
                                let x = (i as isize * SIDE as isize + a) as f64 / span;
                                let y = (j as isize * SIDE as isize + b) as f64 / span;
                                grid.push((2048.0 * x + 4096.0 * y) as i16);
                            }
                        }
                        grids.push(grid);
                    }
                }
            }
        }
        Terrain::build(levels, RADIUS, 1.0, tiles::NO_SEA, &grids)
    }

    /// At a level boundary the colour is one, and that is a consequence rather
    /// than an agreement.
    ///
    /// The whole construction of the rule stands on it being computed in the
    /// **vertex** from two numbers that are equal at a node shared by two
    /// patches of different levels. This is checked piece by piece here: the
    /// node direction (R2b), the slope (R7c), the roughness and the factor
    /// itself -- so when it breaks one day it will be visible **what exactly**
    /// broke, not merely that "a seam appeared".
    ///
    /// The patches are taken deeper than the pyramid on purpose: that is where
    /// the `Terrain::window` windows work, and where two levels read the same
    /// tile with different steps.
    #[test]
    fn a_level_boundary_gets_one_colour_from_both_sides() {
        let terrain = ramp(3);
        let coarse = Patch {
            face: 2,
            level: 4,
            i: 5,
            j: 3,
        };
        // Child (0, 0) covers the parent's nodes [0, SIDE/2]; its node
        // (2a, 2b) coincides with the parent's (a, b).
        let fine = coarse.children()[0];
        let base = detail::base_m(RADIUS);
        let (distance, focal) = (4.0e3, 623.5);

        let mut checked = 0;
        for a in [0usize, 1, 7, SIDE / 4, SIDE / 2] {
            for b in [0usize, 3, SIDE / 4, SIDE / 2] {
                let here = coarse.vertex(a, b, 1.0);
                let there = fine.vertex(2 * a, 2 * b, 1.0);
                assert_eq!(here, there, "node ({a}, {b}) drifted apart geometrically");

                let slope = (
                    terrain.slope_at(&coarse, a, b),
                    terrain.slope_at(&fine, 2 * a, 2 * b),
                );
                assert_eq!(
                    slope.0.to_bits(),
                    slope.1.to_bits(),
                    "slope at node ({a}, {b}): {} against {}",
                    slope.0,
                    slope.1
                );

                let rough = (
                    detail::sample(here, RADIUS, slope.0, base, distance, focal).roughness,
                    detail::sample(there, RADIUS, slope.1, base, distance, focal).roughness,
                );
                assert_eq!(
                    rough.0.to_bits(),
                    rough.1.to_bits(),
                    "roughness at node ({a}, {b})"
                );

                assert_eq!(
                    tint(slope.0, rough.0, false).to_bits(),
                    tint(slope.1, rough.1, false).to_bits(),
                    "factor at node ({a}, {b})"
                );
                checked += 1;
            }
        }
        println!("  {checked} shared level-boundary nodes gave one factor");
        assert!(checked >= 20, "only {checked} nodes were checked");
    }

    /// On flat ground the rule does **nothing** -- and exactly nothing.
    ///
    /// A bitwise one rather than "close to one": on a sea floor the colour must
    /// stay what WAC measured, and a frame with flat terrain must be the same as
    /// before T4.
    ///
    /// Checked at **any** roughness, including values that do not occur on flat
    /// ground: the noise there is zero anyway (its amplitude is proportional to
    /// the slope), but the rule has no right to rely on that -- a term left
    /// outside the multiplication by slope would blotch the sea floor.
    #[test]
    fn flat_ground_keeps_the_colour_the_mosaic_measured() {
        for roughness in [-6.0, -0.5, 0.0, 0.5, 6.0] {
            let got = tint(0.0, roughness, false);
            assert_eq!(
                got.to_bits(),
                1.0f64.to_bits(),
                "flat ground at roughness {roughness} gave {got}"
            );
        }
    }

    /// Under water the rule returns a bitwise one for any input (T7f).
    ///
    /// Bitwise, not "almost": under water the frame shows the surface of the
    /// sea, and the colour there must stay exactly what the mosaic measured.
    /// Checked on the steepest slopes that occur at all -- that is where the
    /// branch is needed, because the ocean floor was measured steeper than the
    /// land.
    #[test]
    fn under_water_the_rule_returns_exactly_one() {
        for slope in [0.0, 0.01, SLOPE_REF, 1.0, 1e6] {
            for roughness in [-6.0, 0.0, 0.5, 6.0] {
                let got = tint(slope, roughness, true);
                assert_eq!(
                    got.to_bits(),
                    1.0f64.to_bits(),
                    "slope {slope}, roughness {roughness} under water gave {got}"
                );
                // And the same point above water is touched by the rule --
                // otherwise the check above would pass with the rule off.
                if slope > 0.0 {
                    assert_ne!(tint(slope, roughness, false), 1.0);
                }
            }
        }
    }

    /// A steeper slope is brighter, and monotonically so.
    #[test]
    fn a_steeper_slope_is_brighter() {
        let mut previous = f64::NEG_INFINITY;
        for step in 0..32 {
            let slope = f64::from(step) * 0.02;
            let got = tint(slope, 0.0, false);
            assert!(
                got >= previous - 1e-15,
                "slope {slope:.2} gave {got:.4} after {previous:.4}"
            );
            previous = got;
        }
        // And the highlight really does saturate where it says it does.
        let at_ref = tint(SLOPE_REF, 0.0, false);
        let beyond = tint(SLOPE_REF * 3.0, 0.0, false);
        assert_eq!(
            at_ref.to_bits(),
            beyond.to_bits(),
            "saturation did not happen"
        );
        assert!((at_ref - (1.0 + SLOPE_GAIN)).abs() < 1e-12);
    }

    /// A crest is brighter than a hollow, and symmetrically so.
    #[test]
    fn a_crest_is_brighter_than_the_hollow_beside_it() {
        let crest = tint(SLOPE_REF, 0.5, false);
        let hollow = tint(SLOPE_REF, -0.5, false);
        println!("  crest {crest:.4}, hollow {hollow:.4}");
        assert!(
            crest > hollow,
            "crest {crest:.4} is not brighter than {hollow:.4}"
        );
        let level = tint(SLOPE_REF, 0.0, false);
        assert!(
            ((crest - level) - (level - hollow)).abs() < 1e-12,
            "relief shifted the mean brightness of the slope"
        );
    }

    /// The relief contrast follows the slope rather than staying constant.
    ///
    /// This is what distinguishes a **material rule** from simply overlaid
    /// noise: colour noise uniform over the whole body would look like a carpet,
    /// and a flat sea floor would be as speckled as a crater wall.
    #[test]
    fn the_relief_contrast_follows_the_slope() {
        let swing = |slope: f64| tint(slope, 0.5, false) - tint(slope, -0.5, false);
        assert_eq!(
            swing(0.0),
            0.0,
            "on flat ground the relief painted nothing at all"
        );

        // Below saturation the swing is proportional to the slope -- not
        // "grows" but exactly proportional. The ratio is compared, so the check
        // does not depend on [`SLOPE_REF`] itself, which is derived from the
        // distribution in the asset and may change with it.
        let unit = swing(SLOPE_REF) / SLOPE_REF;
        for slope in [0.01, 0.05, SLOPE_REF / 2.0, SLOPE_REF] {
            let got = swing(slope) / slope;
            assert!(
                (got - unit).abs() < 1e-12,
                "at slope {slope} the swing per unit slope is {got:.6}, not {unit:.6}"
            );
        }
        println!("  swing per unit slope {unit:.4}, saturation at {SLOPE_REF}");
        // And above saturation it grows no further.
        assert_eq!(swing(SLOPE_REF * 4.0), swing(SLOPE_REF));
    }

    /// The factor never leaves its bounds and never becomes NaN on any input.
    ///
    /// Including inputs that do not occur: a negative slope, a roughness a
    /// hundred times larger than possible. The rule lives in a shader, where
    /// there is nothing to validate input with.
    #[test]
    fn the_tint_never_leaves_its_bounds() {
        for slope in [-1.0, 0.0, 0.01, 0.3, 1.0, 10.0, 1e6] {
            for roughness in [-1e6, -6.0, 0.0, 6.0, 1e6] {
                let got = tint(slope, roughness, false);
                assert!(
                    got.is_finite(),
                    "slope {slope}, roughness {roughness} -> {got}"
                );
                assert!(
                    (MIN_TINT..=MAX_TINT).contains(&got),
                    "slope {slope}, roughness {roughness} -> {got}"
                );
            }
        }
    }
}
