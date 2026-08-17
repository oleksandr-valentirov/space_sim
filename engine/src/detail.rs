//! Procedural surface detail (ROADMAP-PLANETS.md, R7c).
//!
//! The data is finite, the surface is not. The Moon's pyramid has five levels,
//! that is the finest LOLA node is **5330 m**; below that the DEM knows nothing,
//! while the camera comes down to a hundred metres. Interpolating between tile
//! nodes gives no new heights -- it is the same surface on a finer grid. Filling
//! that void is what noise is for.
//!
//! WARNING: **the void would have gone unnoticed without the R7c fix to level
//! selection.** The criterion asked only about the sphere's sagitta, and a
//! sphere is flat up close: a kilometre above the Moon a grid cell came out at
//! 2665 m, that is 1662 pixels wide. Nothing written below would fit in a grid
//! like that.
//!
//! ## Three rules decided before the first line of code
//!
//! **1. The noise is a function of position on the body. Never a function of
//! the frame, of time or of generator state.** Otherwise the same mountain will
//! look different after loading a save, and that will be noticed: a game whose
//! trajectory reproduces bitwise has no right to replay its terrain.
//!
//! **2. The argument is the vertex's unit direction, and that specifically, not
//! a position relative to the patch.** A patch subtracts the camera from its own
//! origin, so "position" in the shader is different for every patch; the
//! direction, meanwhile, lies in the geometry buffer as the normal, and on a
//! shared edge it is **bitwise identical** -- given by the same `Patch::vertex`
//! in `f64`, narrowed to `f32` (R2b). So the continuity of the detail across
//! patch boundaries is not proved but inherited.
//!
//! **3. An octave's fade depends on the positions of the node and the camera --
//! never on the patch level.** Neighbours differ by a level by construction, and
//! a criterion that looks at the level would give two different heights at a
//! shared node. The wavelength in pixels -- `lambda * focal / d` -- does not
//! depend on the level at all.
//!
//! ## Amplitude
//!
//! `k * slope * lambda`: the detail at every scale has the same steepness as the
//! terrain under it. Flat ground stays flat, a crater wall becomes rough -- and
//! that is not taste but what makes the detail a **continuation** of the DEM
//! rather than a carpet over it. The slope comes from
//! [`crate::tiles::Terrain::slope_at`].

/// How many octaves are computed at most.
///
/// Six: from the coarsest octave (3393 m on the Moon) down to 3393/32 ~ 106 m,
/// and the grid allows no deeper -- at the `lod::MAX_LEVEL` ceiling a patch cell
/// is 21 m, so a wave shorter than ~42 m will not fit in it anyway.
pub const OCTAVES: u32 = 6;

/// What fraction of the body's radius the coarsest octave takes.
///
/// **This does not depend on the pyramid depth, and has no right to.** The
/// temptation to take `Terrain::step_m` -- the finest data node, "the detail
/// begins where the data ends" -- looks elegant and breaks rule 1 of the stage:
/// the pyramid depth is a **cooker** parameter, that is generator state.
/// Recooking the asset with one more level would replay the mountains, and the
/// player would see that the landscape they knew became different after a game
/// update.
///
/// Five hundred and twelve gives 3393 m on the Moon -- practically the same
/// scale as the finest LOLA node (5330 m), only derived from the body rather
/// than from the asset.
pub const BASE_DIVISOR: f64 = 512.0;

/// The wavelength of the coarsest octave for a body of this radius.
pub fn base_m(radius_m: f64) -> f64 {
    radius_m / BASE_DIVISOR
}

/// The detail's steepness relative to the terrain's.
///
/// Half the slope: the detail is noticeable but does not turn flat ground into
/// cliffs. The number is deliberately round -- a look parameter rather than a
/// measured quantity, and it can be changed freely while the stage's checks stay
/// green.
pub const STEEPNESS: f64 = 0.5;

/// The wavelength in pixels below which an octave is off entirely.
pub const FADE_LO_PX: f64 = 4.0;
/// The wavelength in pixels above which an octave works at full strength.
pub const FADE_HI_PX: f64 = 16.0;

/// A hash of three integers into `[0, 1)`.
///
/// Integer and deterministic on every platform: `u32` multiplications and shifts
/// are the same everywhere, unlike anything from `libm`. The same kind of
/// decision as banning `libm` in the integration loop, only for a different
/// reason -- not the determinism of physics but that two machines must not see
/// different mountains.
fn hash(x: i32, y: i32, z: i32) -> f64 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    h = h.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 16;
    f64::from(h >> 8) / f64::from(1u32 << 24)
}

/// Hermite smoothing -- `3t^2 - 2t^3`, without a single trigonometric call.
fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise at a point, `[0, 1)`.
///
/// A trilinear blend of the eight corners of a unit grid cell. Value noise
/// rather than gradient noise: it is twice as cheap, and the difference in
/// character disappears as soon as there is more than one octave.
pub fn value_noise(p: [f64; 3]) -> f64 {
    let cell = [p[0].floor(), p[1].floor(), p[2].floor()];
    let t = [
        smooth(p[0] - cell[0]),
        smooth(p[1] - cell[1]),
        smooth(p[2] - cell[2]),
    ];
    let (x, y, z) = (cell[0] as i32, cell[1] as i32, cell[2] as i32);

    let mut out = 0.0;
    for (dx, dy, dz) in [
        (0, 0, 0),
        (1, 0, 0),
        (0, 1, 0),
        (1, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (0, 1, 1),
        (1, 1, 1),
    ] {
        let weight = |t: f64, d: i32| if d == 0 { 1.0 - t } else { t };
        out +=
            hash(x + dx, y + dy, z + dz) * weight(t[0], dx) * weight(t[1], dy) * weight(t[2], dz);
    }
    out
}

/// The weight of an octave with wavelength `wavelength_m` for a node
/// `distance_m` from the camera.
///
/// Depends on exactly three numbers, none of which knows about a patch: the
/// wavelength, the distance and the focal length. So two neighbouring patches of
/// different levels give the same weight at a shared node.
pub fn octave_weight(wavelength_m: f64, distance_m: f64, focal_px: f64) -> f64 {
    let px = wavelength_m / distance_m.max(1.0) * focal_px;
    if px <= FADE_LO_PX {
        return 0.0;
    }
    if px >= FADE_HI_PX {
        return 1.0;
    }
    smooth((px - FADE_LO_PX) / (FADE_HI_PX - FADE_LO_PX))
}

/// What the detail gave at a node: height for the shape and roughness for the
/// material.
///
/// Two numbers from **one** pass over the octaves rather than two passes: the
/// noise is twice as expensive as everything else in the loop, and computing the
/// same eight hashes twice would mean paying as much for the colour as for the
/// terrain.
pub struct Detail {
    /// Surface displacement along the normal, metres.
    pub height_m: f64,
    /// The same noise, weighted **equally across octaves**, dimensionless.
    ///
    /// WARNING: there is exactly one difference from [`Detail::height_m`], and
    /// it is deliberate. Height multiplies each octave by its wavelength --
    /// otherwise small ripples would be as tall as mountains and the surface
    /// would become noise. So in height the coarsest octave weighs 32 times more
    /// than the finest, and a colour derived from it would carry a 3.4 km blob
    /// -- exactly what step T4 was meant to get rid of.
    ///
    /// Colour adds no height, so it needs no weighting by wavelength: every
    /// scale enters equally, the finest included. The noise, the sample points
    /// and the fade are the same -- the colour stays derived from the same shape,
    /// only read with a different weight.
    pub roughness: f64,
}

/// The detail at a node -- both numbers in one pass.
///
/// `unit` is the node's unit direction from the body centre (rule 2 of the
/// module). `base_m` is the coarsest octave's wavelength, [`base_m`] of the
/// body's radius.
pub fn sample(
    unit: [f64; 3],
    radius_m: f64,
    slope: f64,
    base_m: f64,
    distance_m: f64,
    focal_px: f64,
) -> Detail {
    let mut height_m = 0.0;
    let mut roughness = 0.0;
    for octave in 0..OCTAVES {
        let wavelength = base_m / f64::from(1u32 << octave);
        let weight = octave_weight(wavelength, distance_m, focal_px);
        if weight <= 0.0 {
            // Later octaves are only shorter, so they are off too.
            break;
        }
        // One unit of the noise argument is one wavelength on the body's
        // surface.
        let scale = radius_m / wavelength;
        let p = [unit[0] * scale, unit[1] * scale, unit[2] * scale];
        // Noise from [0,1) into [-0.5, 0.5): the detail does not raise the
        // surface overall but ripples it. Otherwise sea level would creep upward
        // with every octave.
        let signed = value_noise(p) - 0.5;
        height_m += signed * STEEPNESS * slope * wavelength * weight;
        // Times 2, so that each octave gives exactly [-1, 1].
        roughness += 2.0 * signed * weight;
    }
    Detail {
        height_m,
        roughness,
    }
}

/// The procedural detail height at a node, metres -- [`sample`] without the
/// second number.
pub fn height_m(
    unit: [f64; 3],
    radius_m: f64,
    slope: f64,
    base_m: f64,
    distance_m: f64,
    focal_px: f64,
) -> f64 {
    sample(unit, radius_m, slope, base_m, distance_m, focal_px).height_m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The noise depends on nothing but position.
    ///
    /// A tautology by the look of it, and that is exactly why the check is
    /// needed: rule 1 of the stage breaks by oversight rather than by decision
    /// -- it is enough for someone to take the frame number as a seed. Here that
    /// becomes a failing test rather than a game whose save reproduces a
    /// different mountain.
    #[test]
    fn the_same_point_always_gives_the_same_height() {
        let unit = [0.267_261_2, 0.534_522_5, 0.801_783_7];
        let first = height_m(unit, 1_737_400.0, 0.05, base_m(1_737_400.0), 2000.0, 623.5);
        for _ in 0..16 {
            let again = height_m(unit, 1_737_400.0, 0.05, base_m(1_737_400.0), 2000.0, 623.5);
            assert_eq!(
                first.to_bits(),
                again.to_bits(),
                "the noise did not reproduce"
            );
        }
        assert!(
            first != 0.0,
            "at this distance the detail should have been non-zero"
        );
    }

    /// The noise is continuous: neighbouring points give neighbouring heights.
    ///
    /// A discontinuity here would mean a crack in the surface, and finding it
    /// later in a shot would be far more expensive. The step is taken much
    /// smaller than the shortest wave, and the height increment must be
    /// proportional to the step.
    #[test]
    fn the_noise_has_no_step_in_it() {
        const RADIUS: f64 = 1_737_400.0;
        const BASE: f64 = 1_737_400.0 / BASE_DIVISOR;
        let finest = BASE / f64::from(1u32 << (OCTAVES - 1));

        let mut worst: f64 = 0.0;
        for k in 0..64 {
            let a = f64::from(k) * 0.7;
            let unit = [a.cos() * 0.6, a.sin() * 0.6, 0.8_f64];
            let n = (unit[0] * unit[0] + unit[1] * unit[1] + unit[2] * unit[2]).sqrt();
            let unit = [unit[0] / n, unit[1] / n, unit[2] / n];

            // A shift of one hundredth of the shortest wave along the
            // surface.
            let step = finest / 100.0 / RADIUS;
            let moved = {
                let v = [unit[0] + step, unit[1], unit[2]];
                let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                [v[0] / n, v[1] / n, v[2] / n]
            };

            let here = height_m(unit, RADIUS, 0.05, BASE, 500.0, 623.5);
            let there = height_m(moved, RADIUS, 0.05, BASE, 500.0, 623.5);
            worst = worst.max((here - there).abs());
        }

        // The steepest octave gives a slope of `STEEPNESS * slope`, so over one
        // hundredth of a wave the increment cannot exceed a few percent of it.
        let bound = STEEPNESS * 0.05 * finest * 0.1;
        println!("  largest increment {worst:.4} m against the bound {bound:.4} m");
        assert!(
            worst < bound,
            "an increment of {worst:.4} m over one hundredth of a wave -- there is a step in the noise"
        );
    }

    /// The detail fades with distance, and fades **monotonically**.
    ///
    /// This is half of the check named in advance in R7: "on receding, the
    /// detail fades rather than flickers". Flicker is non-monotonicity: an
    /// octave that switches back on at a farther camera gives exactly that.
    #[test]
    fn the_detail_fades_out_with_distance() {
        let unit = [0.267_261_2, 0.534_522_5, 0.801_783_7];
        let mut previous = f64::INFINITY;
        let mut zero_at = None;
        for step in 0..40 {
            let distance = 100.0 * 1.4_f64.powi(step);
            let amplitude: f64 = (0..OCTAVES)
                .map(|octave| {
                    let wavelength = base_m(1_737_400.0) / f64::from(1u32 << octave);
                    octave_weight(wavelength, distance, 623.5) * wavelength
                })
                .sum();
            assert!(
                amplitude <= previous + 1e-12,
                "at {distance:.0} m the amplitude grew: {amplitude:.3} after {previous:.3}"
            );
            previous = amplitude;
            if amplitude == 0.0 && zero_at.is_none() {
                zero_at = Some(distance);
            }
        }
        let zero_at = zero_at.expect("the detail must vanish completely at some point");
        println!("  the detail vanishes completely at {zero_at:.3e} m");
        assert!(
            height_m(unit, 1_737_400.0, 0.05, base_m(1_737_400.0), zero_at, 623.5) == 0.0,
            "after a full fade the height must be exactly zero"
        );
    }
}
