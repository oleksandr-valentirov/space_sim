//! How bright is a star against the sky it has to hide in (stage Z, Z4)?
//!
//! Written because the Z4 oracle failed in a way that looked like a bug and
//! was not: a star drawn at `star::MAGNITUDE_ZERO_RADIANCE` survives a daytime
//! sky, and the transmittance the plan meant to hide it with is nowhere near
//! enough. Zenith transmittance is a matter of ten per cent, not of orders.
//!
//!     cargo run --release -p engine --example star_scale

use engine::atmosphere::Model;
use engine::scene::Atmosphere;
use engine::sphere::EARTH_RADIUS_M as BOTTOM;
use engine::star;

fn main() {
    let air = Atmosphere::EARTH;
    let model = Model::build(&air, BOTTOM, 500, [0.1, 0.1, 0.1]);

    println!("zenith transmittance from the ground:");
    let t = model
        .transmittance
        .transmittance_at(&air, BOTTOM, BOTTOM, 1.0);
    println!("  {t:.4?}  -- what Z4 multiplies a star by looking straight up");

    println!();
    println!("sky radiance straight up, by the Sun's height:");
    for (name, mu_s) in [
        ("Sun overhead", 1.0),
        ("Sun at 45 deg", 0.707),
        ("Sun on the horizon", 0.0),
        ("Sun 10 deg below", -0.17),
    ] {
        let sky = model.sky_view(BOTTOM, mu_s, 1.0, 0.0);
        println!("  {name:<20} {:.5?}", sky);
    }

    println!();
    println!("transmittance from 400 km, by how the ray points:");
    let r = BOTTOM + 400.0e3;
    for (name, mu) in [
        ("straight up", 1.0),
        ("horizontal", 0.0),
        ("grazing the air", -0.300),
        ("deeper", -0.310),
        ("deeper still", -0.320),
        ("just missing the ground", -0.336),
    ] {
        let t = model.transmittance.transmittance_at(&air, BOTTOM, r, mu);
        println!("  {name:<26} mu {mu:>7.3}  {t:.5?}");
    }

    println!();
    println!(
        "stars at MAGNITUDE_ZERO_RADIANCE = {}:",
        star::MAGNITUDE_ZERO_RADIANCE
    );
    for magnitude in [0.0f32, 2.0, 4.0, 6.0] {
        let radiance = star::MAGNITUDE_ZERO_RADIANCE * star::flux(magnitude);
        println!("  magnitude {magnitude:>4.1}      {radiance:.5}");
    }
}
