//! The hull material: Cook-Torrance with GGX (ROADMAP, T5c).
//!
//! A literal twin of `ship.slang`, exactly like [`crate::atmosphere`] against
//! `sky.slang` and [`crate::cull`] against `cull.slang`. The oracle is the
//! same: the numbers from both sides must agree, and a test checks that.
//!
//! **Analytic rather than sampled, and that is why it is an oracle.** GGX has
//! a closed form, so the twin gives a number without exposure, tonemapper or
//! any look settings -- so a divergence means an error rather than different
//! settings. Comparison with a Blender render has no such property (ROADMAP,
//! T5).
//!
//! ## The formulation is Karis 2013 / Filament, and every choice has a price
//!
//! - **`alpha = roughness^2`.** Not `roughness` itself: an artistic parameter
//!   must be uniform to the eye rather than in the maths, and the square is the
//!   convention both Blender and glTF understand. Since the parameters will
//!   come from Blender (T5d), taking a different one would silently repaint
//!   every imported material.
//! - **Smith with height correlation**, already divided by `4(n.l)(n.v)`. A
//!   separate G and a separate denominator give zero over zero at grazing
//!   angles -- the same class as `max(x, 1e-30)` in the atmosphere.
//! - **`F0 = 0.04` for a dielectric.** Not a "magic 4%" but the normal
//!   reflectance for a refractive index of ~1.5, that is for paint, glass and
//!   plastic. Metal takes `F0` from the base colour and has no diffuse term at
//!   all.

/// Normal reflectance of a dielectric -- refractive index around 1.5.
pub const DIELECTRIC_F0: f64 = 0.04;

/// The smallest roughness, below which the highlight becomes a delta function.
///
/// WARNING: not cosmetic: as `alpha -> 0` the denominator of `D` tends to zero
/// at a single point, and in `f32` that is infinity in one pixel and zero in
/// the next. The bound is set so the peak of `D` stays well inside `f32`.
pub const MIN_ROUGHNESS: f64 = 0.045;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalise(v: [f64; 3]) -> [f64; 3] {
    let n = dot(v, v).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// Microfacet normal distribution, GGX / Trowbridge-Reitz.
pub fn distribution(n_dot_h: f64, roughness: f64) -> f64 {
    let a = (roughness.max(MIN_ROUGHNESS)).powi(2);
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (std::f64::consts::PI * d * d)
}

/// Smith visibility with height correlation, **already divided** by
/// `4(n.l)(n.v)`.
pub fn visibility(n_dot_v: f64, n_dot_l: f64, roughness: f64) -> f64 {
    let a2 = (roughness.max(MIN_ROUGHNESS)).powi(2).powi(2);
    let v = n_dot_l * (n_dot_v * n_dot_v * (1.0 - a2) + a2).sqrt();
    let l = n_dot_v * (n_dot_l * n_dot_l * (1.0 - a2) + a2).sqrt();
    0.5 / (v + l).max(1e-30)
}

/// Schlick's Fresnel.
pub fn fresnel(f0: f64, v_dot_h: f64) -> f64 {
    f0 + (1.0 - f0) * (1.0 - v_dot_h).clamp(0.0, 1.0).powi(5)
}

/// How much light goes to the eye per unit irradiance -- per channel.
///
/// * `normal`, `view`, `light` are unit vectors; `view` points **from the
///   surface to the eye**, `light` from the surface to the light;
/// * `base` is the channel's base colour, `0..1`;
/// * `roughness`, `metallic` are `0..1`.
///
/// Returns the value already multiplied by `n.l`, that is what goes into the
/// pixel at unit irradiance. Zero when the surface is turned away from the
/// light or from the eye.
pub fn radiance(
    normal: [f64; 3],
    view: [f64; 3],
    light: [f64; 3],
    base: f64,
    roughness: f64,
    metallic: f64,
) -> f64 {
    let n = normalise(normal);
    let v = normalise(view);
    let l = normalise(light);

    let n_dot_l = dot(n, l);
    let n_dot_v = dot(n, v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return 0.0;
    }
    let h = normalise([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    let n_dot_h = dot(n, h).clamp(0.0, 1.0);
    let v_dot_h = dot(v, h).clamp(0.0, 1.0);

    // Metal has no diffuse reflection, and its `F0` is the base colour.
    let f0 = DIELECTRIC_F0 * (1.0 - metallic) + base * metallic;
    let f = fresnel(f0, v_dot_h);
    let specular = distribution(n_dot_h, roughness) * visibility(n_dot_v, n_dot_l, roughness) * f;
    let diffuse = (1.0 - f) * (1.0 - metallic) * base / std::f64::consts::PI;

    (diffuse + specular) * n_dot_l
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mirror direction -- and only it -- gives the peak of the
    /// distribution.
    #[test]
    fn the_lobe_peaks_where_the_mirror_direction_is() {
        for roughness in [0.05, 0.2, 0.5, 1.0] {
            let peak = distribution(1.0, roughness);
            for n_dot_h in [0.0, 0.3, 0.7, 0.9, 0.99] {
                let got = distribution(n_dot_h, roughness);
                assert!(
                    got <= peak,
                    "roughness {roughness}: at n.h = {n_dot_h} the distribution {got} > the peak {peak}"
                );
            }
        }
    }

    /// A smoother surface gives a narrower and taller highlight.
    ///
    /// This is what makes `roughness` a parameter at all: if the peak did not
    /// depend on it monotonically, the slider would mean nothing.
    #[test]
    fn a_smoother_surface_has_a_sharper_highlight() {
        let mut previous = f64::INFINITY;
        for step in 1..20 {
            let roughness = f64::from(step) * 0.05;
            let peak = distribution(1.0, roughness);
            assert!(
                peak < previous,
                "roughness {roughness} gave the peak {peak}, not smaller than {previous}"
            );
            previous = peak;
        }
    }

    /// The distribution is normalised: the integral of `D * (n.h)` over the
    /// hemisphere equals one.
    ///
    /// This is what makes GGX a **distribution** rather than just a bell; an
    /// error in the denominator's exponent or in `alpha^2` breaks exactly this
    /// and nothing else visible.
    #[test]
    fn the_distribution_integrates_to_one() {
        for roughness in [0.1, 0.3, 0.6, 1.0] {
            // The hemisphere in spherical coordinates:
            // integral of D cos(theta) sin(theta) d(theta) d(phi).
            let steps = 20_000;
            let mut sum = 0.0;
            for k in 0..steps {
                let theta = (f64::from(k) + 0.5) / f64::from(steps) * std::f64::consts::FRAC_PI_2;
                sum += distribution(theta.cos(), roughness)
                    * theta.cos()
                    * theta.sin()
                    * (std::f64::consts::FRAC_PI_2 / f64::from(steps));
            }
            let total = sum * 2.0 * std::f64::consts::PI;
            println!("  roughness {roughness}: integral {total:.6}");
            assert!(
                (total - 1.0).abs() < 1e-3,
                "roughness {roughness}: integral {total:.6}, and it must be one"
            );
        }
    }

    /// Fresnel at a grazing angle is full reflection, at the normal it is
    /// `F0`.
    #[test]
    fn fresnel_goes_from_f0_to_one() {
        assert!((fresnel(DIELECTRIC_F0, 1.0) - DIELECTRIC_F0).abs() < 1e-12);
        assert!((fresnel(DIELECTRIC_F0, 0.0) - 1.0).abs() < 1e-12);
        assert!((fresnel(0.9, 0.0) - 1.0).abs() < 1e-12);
    }

    /// Helmholtz reciprocity: swapping the eye with the light changes nothing.
    ///
    /// A physical law rather than a property of the formula -- which is why it
    /// is a check: an asymmetric `visibility` (the classic Smith mistake)
    /// breaks it, and nothing else.
    #[test]
    fn swapping_the_eye_and_the_light_changes_nothing() {
        let normal = [0.0, 0.0, 1.0];
        let mut worst: f64 = 0.0;
        for k in 0..12 {
            for m in 0..12 {
                let a = f64::from(k) * 0.13 + 0.05;
                let b = f64::from(m) * 0.11 + 0.05;
                let view = normalise([a.sin(), 0.0, a.cos()]);
                let light = normalise([b.sin() * 0.6, b.sin() * 0.8, b.cos()]);
                for roughness in [0.1, 0.4, 0.9] {
                    let here = radiance(normal, view, light, 0.5, roughness, 0.0);
                    let there = radiance(normal, light, view, 0.5, roughness, 0.0);
                    // The `n.l` factor is not symmetric, so divide it out.
                    let here = here / dot(normal, light);
                    let there = there / dot(normal, view);
                    worst = worst.max((here - there).abs() / here.max(1e-12));
                }
            }
        }
        println!("  worst asymmetry {worst:.3e}");
        assert!(worst < 1e-12, "reciprocity broken by {worst:.3e}");
    }

    /// Metal has no diffuse reflection, a dielectric has.
    #[test]
    fn metal_reflects_only_its_highlight() {
        let normal = [0.0, 0.0, 1.0];
        // WARNING: the pair of directions must be **far from the mirror one**,
        // and symmetry is a trap here: `v = (s, 0, c)` together with
        // `l = (-s, 0, c)` gives `h = normalize(v + l) = n`, that is exactly
        // the mirror and the peak of the highlight. The first version of this
        // test took that pair and measured metal at its maximum. The hemisphere
        // is spread by the polar angle, not the azimuth.
        let view = normalise([0.174, 0.0, 0.985]);
        let light = normalise([-0.940, 0.0, 0.342]);
        let rough = 0.35;
        let metal = radiance(normal, view, light, 0.9, rough, 1.0);
        let paint = radiance(normal, view, light, 0.9, rough, 0.0);
        println!(
            "  metal {metal:.5}, paint {paint:.5}, by a factor of {:.1}",
            paint / metal
        );
        assert!(
            paint > 4.0 * metal,
            "metal {metal:.5} shines almost like paint {paint:.5} -- the \
             diffuse term was not removed"
        );
    }

    /// A surface turned away from the light or from the eye does not shine.
    #[test]
    fn a_surface_turned_away_is_black() {
        let n = [0.0, 0.0, 1.0];
        let up = [0.0, 0.0, 1.0];
        let down = [0.0, 0.0, -1.0];
        assert_eq!(radiance(n, up, down, 0.8, 0.3, 0.0), 0.0);
        assert_eq!(radiance(n, down, up, 0.8, 0.3, 0.0), 0.0);
    }

    /// The material never gives back more than it got.
    ///
    /// A crude but real bound: the integral of outgoing radiance over the
    /// hemisphere of eye directions cannot exceed one at unit irradiance. A
    /// violation here means the surface emits light of its own.
    ///
    /// WARNING: there is deliberately no lower bound, and the numbers show why:
    /// rough metal gives back only 0.32 of one. That is a **known** property of
    /// single scattering in GGX -- light that bounced twice between microfacets
    /// is not returned by the formula at all -- rather than an error.
    /// Compensation exists (Kulla-Conty), costs one more table and is needed
    /// where rough metal carries the look; a hull at `roughness ~ 0.35` loses
    /// single percent, so we do not pay yet.
    #[test]
    fn the_material_never_gives_back_more_than_it_got() {
        let normal = [0.0, 0.0, 1.0];
        for roughness in [0.08, 0.3, 0.7, 1.0] {
            for metallic in [0.0, 1.0] {
                let light = normalise([0.3, 0.0, 0.954]);
                let steps = 400;
                let mut total = 0.0;
                for k in 0..steps {
                    let theta =
                        (f64::from(k) + 0.5) / f64::from(steps) * std::f64::consts::FRAC_PI_2;
                    let mut ring = 0.0;
                    let around = 200;
                    for m in 0..around {
                        let phi =
                            (f64::from(m) + 0.5) / f64::from(around) * 2.0 * std::f64::consts::PI;
                        let view = [
                            theta.sin() * phi.cos(),
                            theta.sin() * phi.sin(),
                            theta.cos(),
                        ];
                        // Outgoing radiance without the `n.l` factor, times
                        // the cosine of the eye direction -- that is the flux
                        // going out.
                        ring += radiance(normal, view, light, 1.0, roughness, metallic)
                            / dot(normal, light)
                            * theta.cos();
                    }
                    total += ring / f64::from(around)
                        * 2.0
                        * std::f64::consts::PI
                        * theta.sin()
                        * (std::f64::consts::FRAC_PI_2 / f64::from(steps));
                }
                println!("  roughness {roughness}, metallic {metallic}: gave back {total:.4}");
                assert!(
                    total <= 1.0 + 1e-3,
                    "roughness {roughness}, metallic {metallic}: gave back \
                     {total:.4} of one -- the surface emits light of its own"
                );
            }
        }
    }
}
