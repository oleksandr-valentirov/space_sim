//! The tonemapper: compressing luminances above one (ROADMAP, T5c3).
//!
//! The twin of `shaders/tonemap.slang`. The reasons behind every choice are
//! in that file's header; here is what callers and checks need.
//!
//! The main property every oracle of stage T rests on: **below the knee the
//! curve is the identity**, and not "almost" but bitwise. All the diffuse work
//! lives there -- the Moon's reflectance 0.02 to 0.25, the mosaic, the
//! material rule -- so the pass moved none of the already measured numbers.

/// Where the curve stops being the identity.
///
/// 0.8 rather than 1.0: a knee exactly at one would put a derivative break
/// where the eye is most sensitive -- at the edge of a highlight. Eight tenths
/// leaves a fifth of the scale for compression and touches nothing stage T
/// measured.
pub const KNEE: f64 = 0.8;

/// Compress one channel.
pub fn compress(value: f64) -> f64 {
    if value <= KNEE {
        return value;
    }
    let d = 1.0 - KNEE;
    1.0 - d * d / (value - 2.0 * KNEE + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Below the knee the curve does nothing -- bitwise.
    ///
    /// Every oracle of stage T stands on this, including the "byte against
    /// measured reflectance" comparison (T5b). A curve that touched those
    /// values would make that comparison impossible.
    #[test]
    fn below_the_knee_nothing_moves() {
        for k in 0..=800 {
            let value = f64::from(k) / 1000.0;
            assert_eq!(
                compress(value).to_bits(),
                value.to_bits(),
                "{value} moved below the knee"
            );
        }
    }

    /// Above the knee the curve is monotone and never reaches one.
    #[test]
    fn above_the_knee_it_climbs_towards_one_and_never_reaches_it() {
        let mut previous = KNEE;
        for k in 0..2000 {
            let value = KNEE + f64::from(k) * 0.01;
            let got = compress(value);
            assert!(got >= previous, "{value} gave {got} after {previous}");
            assert!(got < 1.0, "{value} gave {got} -- that is already one");
            previous = got;
        }
        // Something very bright must still get close to one, or a highlight
        // comes out grey instead of white.
        assert!(compress(1.0e4) > 0.999);
    }

    /// The curve is smooth at the knee: value and slope agree on both sides.
    ///
    /// A break in the derivative is visible as a ring around a highlight --
    /// exactly the artefact the pass is made against.
    #[test]
    fn the_knee_has_no_corner_in_it() {
        let step = 1e-6;
        let below = (compress(KNEE) - compress(KNEE - step)) / step;
        let above = (compress(KNEE + step) - compress(KNEE)) / step;
        println!("  slope before the knee {below:.6}, after {above:.6}");
        assert!((below - 1.0).abs() < 1e-4);
        assert!((above - 1.0).abs() < 1e-4);
    }

    /// Highlights that used to clip now stay distinct.
    ///
    /// Two luminances, both above one, must give **different** bytes --
    /// otherwise the pass does not do what it exists for. The numbers come
    /// from measurement: `roughness = 0.35` peaks at about 3.7.
    #[test]
    fn two_highlights_that_used_to_clip_are_still_different() {
        let a = crate::srgb::linear_to_byte(compress(2.0));
        let b = crate::srgb::linear_to_byte(compress(3.7));
        let c = crate::srgb::linear_to_byte(compress(12.0));
        println!("  2.0 -> {a}, 3.7 -> {b}, 12.0 -> {c}");
        assert!(
            a < b && b < c,
            "the highlights collapsed together: {a}, {b}, {c}"
        );
        // Without the compression all three would be exactly 255.
        assert_eq!(crate::srgb::linear_to_byte(2.0), 255);
        assert_eq!(crate::srgb::linear_to_byte(3.7), 255);
    }
}
