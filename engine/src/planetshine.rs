//! Planetshine on the hull (ROADMAP, T6).
//!
//! PROJECT.md section 7 demands this outright: ambient in the frame is zero, so
//! the shadowed side of the ship is black -- and must be black -- until a
//! planet appears below it. What lights it is not "a bit of light from
//! everywhere" but a specific body with a specific albedo, which is why this is
//! a computation rather than a constant.
//!
//! ## A disc, not a point, and that is where the number comes from
//!
//! The planet under the ship is not a point source: from low orbit it covers
//! nearly a hemisphere. For a Lambertian disc of half-angle `theta` and
//! constant radiance `L`, the irradiance on a patch facing its centre is
//! `pi*L*sin^2(theta)` -- a closed form, not an approximation.
//!
//! The radiance of a surface at unit irradiance from the light is `A*cos/pi`,
//! where `A` is the albedo. Together: `E = A * cos * sin^2(theta)`, and there
//! is no free factor in that expression.
//!
//! ## Three simplifications, each with a named price
//!
//! **1. A disc replaces the hemisphere.** From an altitude of hundreds of
//! kilometres `theta` reaches 80 degrees, and a disc that large is noticeably
//! not flat. The price is overstated irradiance for patches turned sideways;
//! the correct answer needs integration over the visible cap, that is a table.
//!
//! **2. The source collapses to the direction of the body's centre.** So the
//! hull catches the shine as if it came from one point. For the diffuse term
//! that is a detail, for the specular one it is not: a real reflection of the
//! planet in a polished flank would be a disc, not a point.
//!
//! **3. The illumination is taken at the sub-ship point.** So the terminator is
//! crossed instantly instead of sliding across the disc. The price is visible
//! exactly above the terminator and nowhere else.
//!
//! All three are removed by the same work -- an SH probe integrated over the
//! cap, as PROJECT.md section 7 says. What they have in common is that each is
//! visible only in the **shape** of the disc, that is where there is nothing to
//! measure today: the shine enters the frame as one direction and three
//! numbers.
//!
//! But the **albedo under the ship is no longer simplified**: a body with a
//! colour tileset returns a sample of the asset (`Colour::under`, T6c), so over
//! the sea the ship is lit more weakly than over a continent, and that is a
//! measured number rather than plausibility.

use crate::scene::{Body, Scene};

/// What the planet shines onto the ship.
pub struct Shine {
    /// Direction **to the source**, that is down, at the body's centre; world
    /// axes.
    pub direction: [f64; 3],
    /// Irradiance per channel, in the same units as the light (unity).
    pub irradiance: [f64; 3],
}

impl Shine {
    /// Empty shine -- nowhere and zero.
    pub fn none() -> Shine {
        Shine {
            direction: [0.0, 0.0, 1.0],
            irradiance: [0.0; 3],
        }
    }
}

/// How much body `body` shines onto point `point` with its own colour.
///
/// For a body with a colour tileset the right answer is different -- the albedo
/// there varies from place to place, and [`from_body_albedo`] takes it. The
/// frame calls that one, because the tileset lives in the frame; this one
/// remains for bodies without an asset and for checks of the geometry
/// itself.
pub fn from_body(body: &Body, point: [f64; 3], sun: [f64; 3]) -> Shine {
    let albedo = [
        f64::from(body.colour[0]),
        f64::from(body.colour[1]),
        f64::from(body.colour[2]),
    ];
    from_body_albedo(body, point, sun, albedo)
}

/// The same, but with the surface albedo under the point given from outside.
///
/// Split exactly because **the frame takes albedo from two different places**:
/// a body without a tileset is drawn with its `Body::colour`, a body with one
/// from a sample of the asset, and `Body::colour` then takes no part at all
/// (`surface_albedo` in `patch.slang`). The shine must carry the same albedo
/// the surface is painted with, otherwise the ship would be lit by a planet of
/// one colour above a planet of another.
///
/// WARNING: the material rule (`engine::material`) does not enter here: it
/// multiplies brightness by slope and roughness within +-80%, while the sample
/// here comes from the coarsest pyramid level, where slope of that scale is
/// already gone.
pub fn from_body_albedo(body: &Body, point: [f64; 3], sun: [f64; 3], albedo: [f64; 3]) -> Shine {
    let to_centre = [
        body.centre[0] - point[0],
        body.centre[1] - point[1],
        body.centre[2] - point[2],
    ];
    let distance =
        (to_centre[0] * to_centre[0] + to_centre[1] * to_centre[1] + to_centre[2] * to_centre[2])
            .sqrt();
    if distance <= body.radius_m || distance == 0.0 {
        // Inside the body there is no shine -- there is no ship either.
        return Shine::none();
    }
    let down = [
        to_centre[0] / distance,
        to_centre[1] / distance,
        to_centre[2] / distance,
    ];

    // The disc half-angle and its form factor.
    let sin_theta = body.radius_m / distance;
    let form = sin_theta * sin_theta;

    // Illumination of the sub-ship point: its outward normal is `-down`.
    let lit = -(down[0] * sun[0] + down[1] * sun[1] + down[2] * sun[2]);
    let lit = lit.max(0.0);

    Shine {
        direction: down,
        irradiance: [
            albedo[0] * lit * form,
            albedo[1] * lit * form,
            albedo[2] * lit * form,
        ],
    }
}

/// Shine from the **nearest** body of the scene.
///
/// One body rather than a sum, and that is not an economy: in low orbit the
/// nearest body covers a hemisphere while the rest contribute orders of
/// magnitude less -- Earth from the Moon lights four times more weakly than the
/// Moon does from low orbit, and Jupiter is not visible from Earth at all. A
/// sum will be needed when a scene appears with two bodies at comparable
/// distances.
pub fn nearest(scene: &Scene, point: [f64; 3]) -> Shine {
    match nearest_body(scene, point) {
        Some(k) => from_body(&scene.bodies[k], point, scene.sun),
        None => Shine::none(),
    }
}

/// Which body of the scene is nearest to the point -- by index, not by
/// reference.
///
/// The frame is what needs the index: the tileset lives not in `Body` but in a
/// frame slot (`TileSet::Loaded` is a handle, the engine does not know the
/// asset format), so the answer "here is the body" does not yet give an albedo.
/// By index rather than by reference is the project's style rule (CLAUDE.md).
pub fn nearest_body(scene: &Scene, point: [f64; 3]) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for (k, body) in scene.bodies.iter().enumerate() {
        let d = [
            body.centre[0] - point[0],
            body.centre[1] - point[1],
            body.centre[2] - point[2],
        ];
        let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - body.radius_m;
        if best.is_none_or(|(previous, _)| distance < previous) {
            best = Some((distance, k));
        }
    }
    best.map(|(_, k)| k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::TileSet;

    fn body(colour: [f32; 4]) -> Body {
        Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: 1_000_000.0,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
            colour,
            air: None,
        }
    }

    /// Near the surface the shine tends to the **albedo**, not to something
    /// smaller.
    ///
    /// A number, not "bright": at zero altitude the disc covers the hemisphere,
    /// `sin(theta) -> 1`, and the irradiance equals `A*cos`. An error of `pi` --
    /// the easiest one in this formula -- is made visible here.
    #[test]
    fn just_above_the_surface_the_shine_is_the_albedo_itself() {
        let planet = body([0.4, 0.5, 0.6, 1.0]);
        let sun = [0.0, 0.0, 1.0];
        // Right above the subsolar point: `cos = 1`.
        let point = [0.0, 0.0, planet.radius_m * 1.000_001];
        let shine = from_body(&planet, point, sun);
        println!("  irradiance {:?}", shine.irradiance);
        for channel in 0..3 {
            assert!(
                (shine.irradiance[channel] - f64::from(planet.colour[channel])).abs() < 1e-4,
                "channel {channel}: {} against albedo {}",
                shine.irradiance[channel],
                planet.colour[channel]
            );
        }
    }

    /// With altitude the shine falls off, and falls off as `sin^2(theta)`.
    #[test]
    fn the_shine_falls_off_as_the_disc_shrinks() {
        let planet = body([0.5; 4]);
        let sun = [0.0, 0.0, 1.0];
        let mut previous = f64::INFINITY;
        for step in 0..12 {
            let altitude = planet.radius_m * 0.05 * f64::from(1 << step);
            let distance = planet.radius_m + altitude;
            let shine = from_body(&planet, [0.0, 0.0, distance], sun);
            let expected = 0.5 * (planet.radius_m / distance).powi(2);
            assert!(
                (shine.irradiance[0] - expected).abs() < 1e-12,
                "altitude {altitude}: {} against {expected}",
                shine.irradiance[0]
            );
            assert!(shine.irradiance[0] < previous, "the shine did not fall");
            previous = shine.irradiance[0];
        }
    }

    /// Over the night side there is no shine at all.
    #[test]
    fn over_the_night_side_there_is_nothing() {
        let planet = body([0.8; 4]);
        let sun = [0.0, 0.0, 1.0];
        let distance = planet.radius_m * 1.2;
        for point in [
            [0.0, 0.0, -distance],
            [0.0, distance * 0.7, -distance * 0.7],
        ] {
            let shine = from_body(&planet, point, sun);
            assert_eq!(shine.irradiance, [0.0; 3], "the night side shines");
        }
        // And exactly above the terminator it is exactly zero, not "almost".
        let shine = from_body(&planet, [0.0, distance, 0.0], sun);
        assert_eq!(shine.irradiance, [0.0; 3]);
    }

    /// The shine's colour is the body's colour, not grey.
    ///
    /// Exactly the statement the step is checked by: the light from below must
    /// carry the colour of the surface under the ship.
    #[test]
    fn the_shine_carries_the_colour_of_the_body_below() {
        let sun = [0.0, 0.0, 1.0];
        let point = [0.0, 0.0, 1_200_000.0];
        let blue = from_body(&body([0.2, 0.4, 0.9, 1.0]), point, sun);
        let rust = from_body(&body([0.9, 0.4, 0.2, 1.0]), point, sun);
        println!("  blue {:?}, rust {:?}", blue.irradiance, rust.irradiance);
        assert!(blue.irradiance[2] > 3.0 * blue.irradiance[0]);
        assert!(rust.irradiance[0] > 3.0 * rust.irradiance[2]);
        // And their green is the same -- so the difference is in colour
        // rather than in overall brightness.
        assert!((blue.irradiance[1] - rust.irradiance[1]).abs() < 1e-12);
    }
}
