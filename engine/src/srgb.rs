//! The sRGB transfer function (ROADMAP, T5a).
//!
//! ## Why a module at all, if the hardware does the conversion
//!
//! The frame target is `Rgba8UnormSrgb`, i.e. the hardware encodes on write
//! and there is no encoding anywhere in the shader. But two things remain
//! ours:
//!
//! 1. **Colours picked by eye live in sRGB, while the frame is in linear
//!    light.** Byte 200 in the palette and the number 200/255 in the scene are
//!    different colours the moment the target starts encoding. So whoever puts
//!    a palette colour into the scene must decode it -- exactly once, on the
//!    way in.
//! 2. **Checks read the bytes of a screenshot.** The ratio of two bytes is no
//!    longer the ratio of two luminances, and an oracle that divides bytes
//!    measures gamma. So [`to_linear`] is needed by the tests as much as by
//!    the code.
//!
//! ## The numbers come from the standard, not an approximation
//!
//! There is deliberately no `2.2` here: sRGB is not a power function but a
//! power function with a linear segment near zero, and it is exactly at dark
//! values (where the Moon lives) that the difference is largest. Threshold
//! 0.0031308 / 0.04045, exponent 2.4, factor 1.055.

/// One channel value from sRGB into linear light.
///
/// Input and output are `0..1` rather than bytes: the palette has bytes while
/// the shader and `wgpu::Color` have numbers, and converting between them
/// twice would round twice.
pub fn to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// The other way: linear light into sRGB.
pub fn from_linear(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// An sRGB byte into linear light.
pub fn byte_to_linear(byte: u8) -> f64 {
    to_linear(f64::from(byte) / 255.0)
}

/// Linear light into an sRGB byte -- the same thing the hardware does on
/// write.
///
/// Round to nearest, as in `Rgba8UnormSrgb`; it exists for checks that must
/// say which byte will come out **without drawing a frame**.
pub fn linear_to_byte(value: f64) -> u8 {
    (from_linear(value.clamp(0.0, 1.0)) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ends and the knee -- three points where the standard can be checked
    /// from memory.
    #[test]
    fn the_curve_hits_the_points_the_standard_names() {
        assert_eq!(to_linear(0.0), 0.0);
        assert!((to_linear(1.0) - 1.0).abs() < 1e-12);
        // At the knee both branches must give the same -- otherwise there is a
        // step in the curve, and it falls exactly on the dark tones.
        let low = 0.04045 / 12.92;
        let high = ((0.04045 + 0.055) / 1.055f64).powf(2.4);
        assert!(
            (low - high).abs() < 1e-6,
            "the knee diverged: {low:.9} against {high:.9}"
        );
        // The middle of the scale: 0.5 in sRGB is about 0.214 linear, a number
        // worth recognising on sight.
        assert!((to_linear(0.5) - 0.2140).abs() < 1e-3, "{}", to_linear(0.5));
    }

    /// The forward and inverse transforms undo each other.
    #[test]
    fn the_two_directions_undo_each_other() {
        let mut worst: f64 = 0.0;
        for k in 0..=1000 {
            let v = f64::from(k) / 1000.0;
            worst = worst.max((from_linear(to_linear(v)) - v).abs());
            worst = worst.max((to_linear(from_linear(v)) - v).abs());
        }
        println!("  worst round-trip error {worst:.3e}");
        assert!(worst < 1e-12, "the round trip does not close: {worst:.3e}");
    }

    /// Every byte survives the round trip unshifted.
    ///
    /// This is the claim `frame::CLEAR` rests on: a colour given in bytes and
    /// decoded into linear light will be written back by the hardware as the
    /// same bytes. If rounding lost a unit somewhere, the frame's sky would
    /// drift.
    #[test]
    fn every_byte_survives_the_round_trip() {
        for byte in 0..=255u8 {
            let back = linear_to_byte(byte_to_linear(byte));
            assert_eq!(back, byte, "byte {byte} came back as {back}");
        }
    }

    /// Dark tones are exactly where the 2.2 power approximation lies most.
    ///
    /// The check exists as a guard against "simplification": `powf(2.2)` looks
    /// harmless and is a quarter of the value off at byte 20.
    #[test]
    fn the_linear_toe_is_not_a_power_of_2_2() {
        let byte = 20u8;
        let exact = byte_to_linear(byte);
        let crude = (f64::from(byte) / 255.0).powf(2.2);
        println!("  byte {byte}: exactly {exact:.6}, through 2.2 {crude:.6}");
        assert!(
            (exact - crude).abs() / exact > 0.1,
            "the difference is too small for the check to guard anything"
        );
    }
}
