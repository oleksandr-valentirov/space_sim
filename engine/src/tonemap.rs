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

/// The exposure a scene carries until someone sets another one (step Z1).
///
/// Exactly one, and that number is load-bearing: at one the multiplier is a
/// no-op, the curve stays the identity below the knee, and every frame drawn
/// before Z1 comes out bit for bit the same. Every oracle of stage T rests on
/// that -- T5b compares a byte against a measured reflectance.
pub const DEFAULT_EXPOSURE: f64 = 1.0;

/// One channel through the whole pass: exposure, then the curve.
///
/// The multiplier goes **before** the curve, never after. After it, it would
/// stretch numbers the curve has already flattened -- the highlight would come
/// back as a white blob, which is the one thing the pass exists to prevent.
///
/// There is no automatic exposure and there is not meant to be one. A factor
/// that drifted with the contents of the frame would dim the faint exactly
/// when something bright entered it: stage Y's night lights would go out at
/// the terminator, the one place they are supposed to appear. The scene says
/// the number.
pub fn expose(value: f64, exposure: f64) -> f64 {
    compress(value * exposure)
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

    /// At the default exposure the pass does nothing at all -- bitwise.
    ///
    /// The first oracle of Z1, and the more important of the two. Every frame
    /// drawn before exposure existed has to come out the same byte for byte,
    /// or Z1 has quietly re-based every measured number of stage T. Bits, not
    /// a tolerance: a multiplier by one is exact in IEEE 754 for every finite
    /// value, so anything less than equality here means the multiplier landed
    /// somewhere it should not have.
    #[test]
    fn the_default_exposure_moves_nothing() {
        for k in 0..=4000 {
            let value = f64::from(k) / 1000.0;
            assert_eq!(
                expose(value, DEFAULT_EXPOSURE).to_bits(),
                compress(value).to_bits(),
                "{value} moved at the default exposure"
            );
        }
    }

    /// More exposure never makes a pixel darker.
    ///
    /// The second oracle. The curve is monotone and so is multiplication by a
    /// positive number, but the two are composed here, and a multiplier put
    /// after the curve instead of before would still pass a "brighter is
    /// brighter" eyeball test while destroying the highlight. This checks the
    /// composition across the knee, where the curve bends.
    #[test]
    fn more_exposure_never_darkens_a_pixel() {
        for k in 0..=400 {
            let value = f64::from(k) / 100.0;
            let mut previous = 0.0;
            for e in 1..=40 {
                let exposure = f64::from(e) / 4.0;
                let got = expose(value, exposure);
                assert!(
                    got >= previous,
                    "{value} at exposure {exposure} gave {got} after {previous}"
                );
                previous = got;
            }
        }
        // And the point of it all: a disc far brighter than the surface keeps
        // a colour of its own instead of clipping to white with it. Halving
        // the exposure has to pull the two apart in bytes.
        let disc = crate::srgb::linear_to_byte(expose(40.0, 0.5));
        let ground = crate::srgb::linear_to_byte(expose(1.2, 0.5));
        println!("  disc -> {disc}, ground -> {ground}");
        assert!(disc > ground, "the disc and the ground merged: {disc}");
    }
}
