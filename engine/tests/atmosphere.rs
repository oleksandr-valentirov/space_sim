//! The transmittance table agrees with the oracle (ROADMAP-ATMOSPHERE.md, S2).
//!
//! ## What is proved here
//!
//! Rule 2 of stage S: every LUT gets a **number**, not "looks like sky". The
//! number comes from `engine::atmosphere` -- a separate implementation of the
//! same physics in `f64` on the CPU -- and the oracle itself is pinned down by
//! a closed form in its own unit tests. The chain in full:
//!
//! 1. `beta*H*(exp(-h0/H) - exp(-h1/H))` vs `atmosphere::optical_depth`
//!    (`engine::atmosphere::tests`, no GPU);
//! 2. `atmosphere::optical_depth` vs the table on the GPU -- **here**.
//!
//! Both links are needed. Without the first, two numeric integrations would
//! agree on a shared mistake; without the second the shader is not checked at
//! all.
//!
//! ## Why "over dozens of altitudes and angles" rather than at one point
//!
//! A mistake in the parametrisation gives the right number precisely where
//! `u = 0` -- the vertical falls out of it by itself. A mistake in the ozone
//! is visible only in the 10-40 km layer. A mistake in the geometry only at a
//! large angle, where the ray is long. One point sees none of the three.
//!
//! ## Where a tolerance cannot be relative, and why
//!
//! A shadow is a **step**: `lit` means "the ray to the Sun did not meet the
//! surface", i.e. a step rather than a smooth function. Near the terminator
//! one ULP in the geometry tips a whole march sample from one side of the step
//! to the other, and two different devices have every right to decide
//! differently. Where the value itself is made of several such samples -- the
//! night side, units of 1e-6 -- the difference comes out on the order of the
//! value, and no percentage describes it.
//!
//! That is why both tolerances carry an absolute term of 3e-6, and it is not
//! about half-float. This was caught on CI (llvmpipe) where NVIDIA agreed; the
//! same class as "you do not check a tie on the GPU" in F3, only the step is
//! in the shadow rather than in the depth.
//! WARNING: locally this class is not caught at all: lavapipe on the
//! developer's machine does not create a device ("Parent device is lost"), so
//! this oracle has its second device only in CI.

use engine::atmosphere;
use engine::gpu::Gpu;
use engine::scene::Atmosphere;
use engine::sky::Sky;

const BOTTOM: f64 = 6_371_000.0;

/// The albedo of the ground under the sky -- the measured mean of
/// `assets/earth.col` (T7h).
///
/// Non-zero deliberately: with a zero, comparing the GPU against the twin
/// would not check the reflection term at all, because it would vanish
/// identically in both branches.
const ALBEDO: [f32; 3] = [0.0595, 0.0595, 0.0732];

/// The same number for the twin, which computes in `f64`.
fn twin_albedo() -> [f64; 3] {
    ALBEDO.map(f64::from)
}

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// The table on the GPU and the oracle on the CPU give the same
/// transmittance.
///
/// **All** 16 384 texels are compared, not a sample: the table is small, and a
/// mistake living in one corner is exactly what a sample does not see.
///
/// The tolerance is on transmittance rather than on optical depth, and that is
/// deliberate: it is transmittance that goes into the frame, so the error has
/// to be measured where it has an effect.
#[test]
fn the_transmittance_table_matches_the_oracle_everywhere() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    assert!(
        sky.ensure(&gpu, &air, BOTTOM, ALBEDO),
        "the first time the table is computed"
    );

    let table = sky
        .read_transmittance(&gpu)
        .expect("the table should have been read");
    let width = atmosphere::TRANSMITTANCE_WIDTH;
    let height = atmosphere::TRANSMITTANCE_HEIGHT;
    assert_eq!(table.len(), (width * height) as usize);

    let mut worst = 0.0f64;
    let mut worst_at = (0u32, 0u32, 0usize);
    for y in 0..height {
        for x in 0..width {
            // The ends of the unit range sit at the centres of the edge
            // texels, as in the shader: divide by `size - 1`, not by the size.
            let u = f64::from(x) / f64::from(width - 1);
            let v = f64::from(y) / f64::from(height - 1);
            let (r, mu) = atmosphere::uv_to_r_mu(&air, BOTTOM, u, v);
            let expected = atmosphere::transmittance(&air, BOTTOM, r, mu, atmosphere::ORACLE_STEPS);
            let got = table[(y * width + x) as usize];
            for channel in 0..3 {
                let difference = (f64::from(got[channel]) - expected[channel]).abs();
                if difference > worst {
                    worst = difference;
                    worst_at = (x, y, channel);
                }
            }
        }
    }

    // 1e-3 is a measured ceiling, not a round number off the top of the head.
    // It is made of two terms, and the larger one here is not the one you
    // would think: the integration step (500 against the oracle's 2048) gives
    // 3.6e-5 even on the worst ray of the table, and the rest is **storage**:
    // a half-float has 11 significant bits, i.e. a step of 5e-4 near one. So
    // the table is limited by the format, not by the arithmetic, and giving
    // the shader more steps would buy nothing.
    assert!(
        worst < 1.0e-3,
        "worst divergence {worst} at texel {worst_at:?}"
    );
}

/// The first column of the table is the vertical ray, and its oracle is
/// closed-form.
///
/// Separate from the test above, because here the comparison is **not against
/// numeric integration at all**: `vertical_optical_depth` is a formula. So the
/// shader is checked against arithmetic that has no integration step in it.
#[test]
fn the_vertical_column_matches_the_closed_form() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    sky.ensure(&gpu, &air, BOTTOM, ALBEDO);
    let table = sky
        .read_transmittance(&gpu)
        .expect("the table should have been read");
    let width = atmosphere::TRANSMITTANCE_WIDTH;

    let mut worst = 0.0f64;
    for y in 0..atmosphere::TRANSMITTANCE_HEIGHT {
        let v = f64::from(y) / f64::from(atmosphere::TRANSMITTANCE_HEIGHT - 1);
        let (r, mu) = atmosphere::uv_to_r_mu(&air, BOTTOM, 0.0, v);
        // Exactly the vertical to within `f64` rounding, not "almost": that is
        // what the ends of the unit range sitting at the centres of the edge
        // texels is for. Before S2 the texel closest to the vertical had
        // `mu = 0.98`.
        assert!(
            (mu - 1.0).abs() < 1.0e-12,
            "row {y}: column 0 should look straight up, but mu = {mu}"
        );

        let closed = atmosphere::vertical_optical_depth(&air, BOTTOM, r);
        let got = table[(y * width) as usize];
        for channel in 0..3 {
            let expected = (-closed[channel]).exp();
            worst = worst.max((f64::from(got[channel]) - expected).abs());
        }
    }
    assert!(worst < 2.0e-3, "worst divergence from the formula {worst}");
}

/// Transmittance grows with altitude and falls as the ray tilts.
///
/// Two monotonicities that depend on no number at all and catch swapped table
/// axes -- a mistake a tolerance cannot see, because after a swap both sides
/// of the comparison read the same wrong texel.
#[test]
fn the_table_is_monotone_in_both_of_its_axes() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    sky.ensure(&gpu, &air, BOTTOM, ALBEDO);
    let table = sky
        .read_transmittance(&gpu)
        .expect("the table should have been read");
    let width = atmosphere::TRANSMITTANCE_WIDTH;
    let height = atmosphere::TRANSMITTANCE_HEIGHT;
    let at = |x: u32, y: u32| f64::from(table[(y * width + x) as usize][2]);

    // Higher up is clearer: less air is left overhead.
    for y in 1..height {
        assert!(
            at(0, y) >= at(0, y - 1) - 1.0e-6,
            "row {y}: {} against {}",
            at(0, y),
            at(0, y - 1)
        );
    }
    // Shallower is darker: the ray goes through a longer path.
    for x in 1..width {
        assert!(
            at(x, height / 2) <= at(x - 1, height / 2) + 1.0e-6,
            "column {x}: {} against {}",
            at(x, height / 2),
            at(x - 1, height / 2)
        );
    }
}

/// The table is not recomputed while the air is the same, and is recomputed
/// when it is not.
///
/// Rule 5 of stage S says transmittance is computed "once and for all". A
/// claim easy to write in a comment and hard to notice broken -- so here it
/// is.
#[test]
fn the_table_is_recomputed_only_when_the_air_changes() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    assert!(
        sky.ensure(&gpu, &air, BOTTOM, ALBEDO),
        "the first time we compute"
    );
    assert!(
        !sky.ensure(&gpu, &air, BOTTOM, ALBEDO),
        "the same air -- we do not compute"
    );

    // A different radius for the same body means different air: the altitude
    // above the surface is counted from it.
    assert!(
        sky.ensure(&gpu, &air, BOTTOM + 1000.0, ALBEDO),
        "a different radius -- a different table"
    );

    let mut thicker = air;
    thicker.rayleigh_height_m *= 2.0;
    assert!(
        sky.ensure(&gpu, &thicker, BOTTOM + 1000.0, ALBEDO),
        "different air"
    );

    // And the albedo is in the key too: it changes the multiple-scattering
    // table, so a rebuild has to happen for it alone as well (T7h).
    assert!(
        sky.ensure(&gpu, &thicker, BOTTOM + 1000.0, [0.5, 0.5, 0.5]),
        "a different albedo -- a different table"
    );
    assert!(
        !sky.ensure(&gpu, &thicker, BOTTOM + 1000.0, [0.5, 0.5, 0.5]),
        "the same albedo -- we do not compute twice"
    );
}

/// The table sizes are written down in both Rust and Slang -- and have to
/// match.
///
/// There is no constant shared between them, so a guard that greps the shader
/// file compares them. The same trick as with `SIDE` for patches (R6a).
#[test]
fn the_table_size_is_the_same_on_both_sides() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders/sky.slang"),
    )
    .expect("the shader should have been read");

    for (name, value) in [
        ("TRANSMITTANCE_WIDTH", atmosphere::TRANSMITTANCE_WIDTH),
        ("TRANSMITTANCE_HEIGHT", atmosphere::TRANSMITTANCE_HEIGHT),
        ("MULTISCATTER_SIZE", atmosphere::MULTISCATTER_SIZE),
        (
            "MULTISCATTER_DIRECTIONS",
            atmosphere::MULTISCATTER_DIRECTIONS,
        ),
        ("MULTISCATTER_STEPS", atmosphere::MULTISCATTER_STEPS),
        ("SKYVIEW_WIDTH", atmosphere::SKYVIEW_WIDTH),
        ("SKYVIEW_HEIGHT", atmosphere::SKYVIEW_HEIGHT),
        ("SKYVIEW_STEPS", atmosphere::SKYVIEW_STEPS),
    ] {
        let wanted = format!("static const uint {name} = {value}u;");
        assert!(
            source.contains(&wanted),
            "sky.slang has no line \"{wanted}\""
        );
    }
}

// ---------------------------------------------------------------------------
// S3 -- multiple scattering
// ---------------------------------------------------------------------------

/// The table and the CPU twin give the same `psi`.
///
/// The twin reads **its own** transmittance table, built in `f64`, not the one
/// on the GPU. So the error of the table itself enters the comparison, and
/// that is deliberate: in the frame the shader will read a table rather than
/// an integral, and what has to be checked is the path the sky is really drawn
/// by.
#[test]
fn the_multiscatter_table_matches_the_oracle() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    sky.ensure(&gpu, &air, BOTTOM, ALBEDO);
    let table = sky
        .read_multiscatter(&gpu)
        .expect("the table should have been read");
    let size = atmosphere::MULTISCATTER_SIZE;
    assert_eq!(table.len(), (size * size) as usize);

    // The transmittance table for the twin uses the same 500 steps as the
    // shader: what is checked here is the scattering, and the precision of
    // transmittance has already been checked above and separately.
    let transmittance = atmosphere::Table::transmittance(&air, BOTTOM, 500);

    let mut worst = 0.0f64;
    let mut worst_at = (0u32, 0u32);
    let mut largest = 0.0f64;
    for y in 0..size {
        for x in 0..size {
            let u = f64::from(x) / f64::from(size - 1);
            let v = f64::from(y) / f64::from(size - 1);
            let (r, mu_s) = atmosphere::multiscatter_uv(&air, BOTTOM, u, v);
            let (psi, _) = atmosphere::multiple_scattering(
                &air,
                BOTTOM,
                &transmittance,
                r,
                mu_s,
                twin_albedo(),
            );
            let got = table[(y * size + x) as usize];
            for channel in 0..3 {
                largest = largest.max(psi[channel]);
                let expected = psi[channel];
                let difference = (f64::from(got[channel]) - expected).abs();
                // The tolerance has two terms, because there are two sources
                // of error, and at the two ends of the table different ones
                // dominate.
                //
                // 1% is **arithmetic**: the twin reads its own transmittance
                // table in `f64`, the shader reads its own in half-float, and
                // the difference of the samples passes through all 64
                // directions. Measured: by day the divergence is 0.1%, i.e. a
                // tenth of the tolerance.
                //
                // 3e-6 is **a discontinuity, not rounding**, and that is the
                // main thing to read here. At night `psi` falls to 1e-6 and is
                // made of a few samples whose contribution is decided by a
                // **step**: `lit` is "the ray to the Sun did not meet the
                // surface", i.e. a step rather than a smooth function. Near
                // the terminator one ULP in the geometry tips a whole sample
                // from one side of the step to the other, and two different
                // devices have every right to decide differently. There is no
                // relative tolerance for this: the difference there is on the
                // order of the value itself.
                //
                // Caught on CI (llvmpipe) where NVIDIA agreed: texel (13, 16),
                // i.e. the Sun 9 deg below the horizon. The same class as "you
                // do not check a tie on the GPU" in F3, only the step here is
                // in the shadow rather than in the depth.
                let allowed = 3.0e-6 + 0.01 * expected.max(f64::from(got[channel]));
                if difference - allowed > worst {
                    worst = difference - allowed;
                    worst_at = (x, y);
                }
            }
        }
    }

    assert!(largest > 0.0, "the table is empty -- all zeroes");
    assert!(
        worst <= 0.0,
        "texel {worst_at:?} exceeds the tolerance by {worst}"
    );
}

/// The energy does not grow: the scattering series converges everywhere.
///
/// `psi = L2/(1 - f)` only makes sense while `f < 1`; at `f >= 1` every
/// further scattering would add no less than the one before, and the sum would
/// diverge. This is the step's oracle, named in ROADMAP-ATMOSPHERE.md, and it
/// is exactly what `f` lives in the table's alpha for.
#[test]
fn every_further_scattering_adds_less_than_the_one_before() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    sky.ensure(&gpu, &air, BOTTOM, ALBEDO);
    let table = sky
        .read_multiscatter(&gpu)
        .expect("the table should have been read");

    let mut largest_fraction = 0.0f32;
    for (index, texel) in table.iter().enumerate() {
        let fraction = texel[3];
        assert!(
            (0.0..1.0).contains(&fraction),
            "texel {index}: fraction {fraction} -- the series does not converge"
        );
        largest_fraction = largest_fraction.max(fraction);
        for (channel, value) in texel.iter().enumerate().take(3) {
            assert!(
                value.is_finite() && *value >= 0.0,
                "texel {index}, channel {channel}: {value}"
            );
        }
    }
    // Measured: the largest fraction is noticeably below one, i.e. Earth's air
    // does not come close to the convergence limit. The number here is a guard
    // for the case where somebody raises the scattering and quietly
    // approaches it.
    assert!(
        largest_fraction < 0.5,
        "the largest fraction is {largest_fraction} -- suspiciously close to \
         the limit"
    );
}

/// More sun means no less light; above the peak means less scattered light.
///
/// Two properties of the axes that catch a swap between them: after a swap the
/// agreement with the twin would survive (both read the same wrong texel), but
/// these would not.
///
/// **The second property is not "falls monotonically", and that is measured
/// rather than simplified.** The altitude profile has a maximum at ~6 km: right
/// at the surface the lower hemisphere emits nothing (albedo zero, S3), so
/// there is less scattered light there than a few kilometres higher. Beyond
/// that the air runs out and `psi` falls monotonically all the way to the top
/// boundary. Writing "falls everywhere" here would mean fitting the claim to
/// convenience.
#[test]
fn the_multiscatter_table_is_monotone_in_both_of_its_axes() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    sky.ensure(&gpu, &air, BOTTOM, ALBEDO);
    let table = sky
        .read_multiscatter(&gpu)
        .expect("the table should have been read");
    let size = atmosphere::MULTISCATTER_SIZE;
    let at = |x: u32, y: u32| f64::from(table[(y * size + x) as usize][2]);

    // The higher the Sun above the horizon, the no less scattered light there
    // is.
    for y in 0..size {
        for x in 1..size {
            assert!(
                at(x, y) >= at(x - 1, y) * 0.999,
                "row {y}, column {x}: {} against {}",
                at(x, y),
                at(x - 1, y)
            );
        }
    }

    // The altitude profile at noon. It is **not monotone**, and that is not
    // noise -- it is the ozone, and that is exactly why the claim here is so
    // small.
    //
    // Measured: a maximum at 6.5 km (right at the surface the lower hemisphere
    // emits nothing -- albedo zero, S3), then a decline, a dip at 35 km --
    // there the ozone layer eats what would otherwise have scattered -- then a
    // **second rise** to 58 km, already above the layer, and finally a decline
    // to the top boundary, where there is nothing left to scatter off. Writing
    // "falls with altitude" here would have been simpler and untrue.
    let noon: Vec<f64> = (0..size).map(|y| at(size - 1, y)).collect();
    let peak = noon
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)
        .expect("the column is not empty");
    assert!(
        peak < (size / 8) as usize,
        "the scattering peak is at row {peak} of {size} -- that is no longer \
         the surface layer"
    );

    // The dip in the ozone layer: the minimum between 20 and 50 km is lower
    // than both what is below it and what is above it.
    let layer = 6..16;
    let dip = layer
        .clone()
        .min_by(|a, b| noon[*a].total_cmp(&noon[*b]))
        .expect("the range is not empty");
    assert!(
        noon[dip] < noon[4] && noon[dip] < noon[18],
        "there is no dip in the ozone layer: {} against {} below and {} above",
        noon[dip],
        noon[4],
        noon[18]
    );

    // But the air does run out: at the top boundary there is half as much
    // scattered light as at the peak. That is what a swap of the axes would
    // break.
    assert!(
        noon[(size - 1) as usize] < noon[peak] * 0.5,
        "at the top boundary {} against the peak {}",
        noon[(size - 1) as usize],
        noon[peak]
    );
}

// ---------------------------------------------------------------------------
// S4 -- the sky
// ---------------------------------------------------------------------------

/// The ground under the sky makes the sky brighter -- and by exactly how much
/// (T7h).
///
/// Before T7h the surface albedo was a zero, and that was a decision: the
/// surface colour did not exist in the scene at all, so any number would have
/// been made up. Now it comes from the tileset (`Colour::mean`), and this test
/// says the term is not merely present in the code but reaches the brightness.
///
/// The twin rather than the GPU: the question here is physical -- "is it
/// brighter" -- and it is answered by the same arithmetic the shader is
/// already verified to reproduce
/// (`the_multiscatter_table_matches_the_oracle` runs both branches with a
/// **non-zero** albedo).
#[test]
fn the_ground_under_the_sky_makes_it_brighter() {
    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let dark = atmosphere::Model::build(&air, BOTTOM, 500, [0.0; 3]);
    let lit = atmosphere::Model::build(&air, BOTTOM, 500, twin_albedo());

    // The zenith from sea level, the Sun high: that is where multiple
    // scattering weighs most, i.e. where light reflected from below has room
    // to show.
    let r = BOTTOM + 2.0;
    let mut worst = f64::INFINITY;
    let mut best: f64 = 0.0;
    for mu_s in [1.0, 0.7, 0.4] {
        for mu_v in [1.0, 0.5, 0.1] {
            let a = dark.sky_view(r, mu_s, mu_v, 0.0);
            let b = lit.sky_view(r, mu_s, mu_v, 0.0);
            for channel in 0..3 {
                assert!(
                    b[channel] >= a[channel],
                    "albedo {:.3} made the sky darker at mu_s {mu_s}, mu_v \
                     {mu_v}, channel {channel}: {} against {}",
                    twin_albedo()[channel],
                    b[channel],
                    a[channel]
                );
                let gain = b[channel] / a[channel].max(1.0e-30);
                worst = worst.min(gain);
                best = best.max(gain);
            }
        }
    }
    println!("  the sky brightens by {worst:.4}...{best:.4} times at albedo {ALBEDO:?}");

    // And the gain is noticeable: a term lost among zeroes would have given
    // exactly one.
    assert!(
        best > 1.005,
        "the largest gain is only {best:.5} -- the term never arrived anywhere"
    );
}

/// The sky table agrees with the twin, and from three different cameras.
///
/// Three, not one: the parametrisation along the zenith depends on the
/// altitude (the horizon from ten kilometres up is lower than from sea level),
/// and the distribution of light depends on where the Sun is. One camera would
/// check one diagonal of the table.
#[test]
fn the_sky_table_matches_the_oracle_from_three_cameras() {
    let Some(gpu) = gpu() else { return };

    let air = Atmosphere::EARTH.with_surface(BOTTOM);
    let mut sky = Sky::new(&gpu, engine::shot::FORMAT);
    sky.ensure(&gpu, &air, BOTTOM, ALBEDO);

    // 500 steps in the transmittance table -- exactly as many as in the
    // shader: what is checked here is the sky, and the precision of
    // transmittance has already been checked separately.
    let model = atmosphere::Model::build(&air, BOTTOM, 500, twin_albedo());

    let width = atmosphere::SKYVIEW_WIDTH;
    let height = atmosphere::SKYVIEW_HEIGHT;

    // Noon from sea level, sunset from sea level, noon from twenty kilometres.
    // `mu_s = 0` puts the Sun exactly on the geometric horizon.
    for (label, altitude, mu_s) in [
        ("noon from the ground", 2.0, 1.0f64),
        ("sunset from the ground", 2.0, 0.0),
        ("noon from 20 km", 20_000.0, 1.0),
    ] {
        let r = BOTTOM + altitude;
        // The sky table reads exactly two numbers off the camera -- the radius
        // and `mu_s` -- so the rest of `View` here is arbitrary as long as it
        // is consistent: the Sun at the zenith along `z`, the camera on `z`
        // too, and `mu_s` follows by itself.
        let view = engine::sky::View {
            eye: [0.0, 0.0, r],
            sun: [(1.0 - mu_s * mu_s).sqrt() as f32, 0.0, mu_s as f32],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            tan_half: [1.0, 0.5],
        };
        assert!(
            (view.sun_zenith_cos() - mu_s).abs() < 1.0e-9,
            "the fixture does not give {mu_s}"
        );
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        sky.prepare_view(&gpu, &mut encoder, &view);
        gpu.queue.submit([encoder.finish()]);

        let table = sky
            .read_skyview(&gpu)
            .expect("the table should have been read");
        assert_eq!(table.len(), (width * height) as usize);

        let mut worst = 0.0f64;
        let mut worst_at = (0u32, 0u32);
        let mut largest = 0.0f64;
        // Every fourth texel: the twin runs 32 steps per ray and reads two
        // tables, so a full grid would cost minutes. A step of 4 leaves 1296
        // directions -- two orders more than needed to see a mistake in the
        // parametrisation.
        for y in (0..height).step_by(4) {
            for x in (0..width).step_by(4) {
                let u = f64::from(x) / f64::from(width - 1);
                let v = f64::from(y) / f64::from(height - 1);
                let (mu_v, cos_azimuth) = atmosphere::skyview_uv(BOTTOM, r, u, v);
                let expected = model.sky_view(r, mu_s, mu_v, cos_azimuth);
                let got = table[(y * width + x) as usize];
                for channel in 0..3 {
                    largest = largest.max(expected[channel]);
                    // The same composition of tolerance as in S3, and the
                    // absolute term is the same for the same reason: **the
                    // shadow step**, not rounding. Directions below the horizon
                    // towards the Sun carry units of 1e-6, and in them what
                    // decides is which side of the terminator an individual
                    // sample landed on; llvmpipe and NVIDIA decide differently,
                    // and both are right.
                    //
                    // The relative term is larger than in S3 (5% against 1%),
                    // and also for a stated reason: along the ray **two**
                    // tables are read, both on the GPU in half-float, and
                    // their errors add up.
                    let allowed = 3.0e-6 + 0.05 * expected[channel].max(f64::from(got[channel]));
                    let difference = (f64::from(got[channel]) - expected[channel]).abs();
                    if difference - allowed > worst {
                        worst = difference - allowed;
                        worst_at = (x, y);
                    }
                }
            }
        }
        assert!(
            largest > 1.0e-4,
            "{label}: the table is dark, the largest is {largest}"
        );
        assert!(
            worst <= 0.0,
            "{label}: texel {worst_at:?} exceeds the tolerance by {worst}"
        );
    }
}

/// The forward and inverse transforms of the sky axes give the same point.
///
/// Not a tautology: the forward one reads the table, the inverse writes it,
/// and it is precisely a divergence between them that would give a sky offset
/// against the Sun -- a mistake visible in the frame as "the sunset is in the
/// wrong place".
#[test]
fn the_sky_parametrisation_survives_a_round_trip() {
    for altitude in [2.0, 10_000.0, 90_000.0] {
        let r = BOTTOM + altitude;
        let mut worst: f64 = 0.0;
        for i in 0..24 {
            for j in 0..24 {
                let u = f64::from(i) / 23.0;
                let v = f64::from(j) / 23.0;
                let (mu_v, cos_azimuth) = atmosphere::skyview_uv(BOTTOM, r, u, v);
                let (u2, v2) = atmosphere::skyview_coords(BOTTOM, r, mu_v, cos_azimuth);
                worst = worst.max((u - u2).abs()).max((v - v2).abs());
            }
        }
        assert!(worst < 1.0e-6, "altitude {altitude}: mismatch {worst}");
    }
}

/// The horizon sits exactly in the middle of the table, and it drops with
/// height.
///
/// This is the property the zenith axis is non-linear for: half the rows go to
/// the sky, half to what is below the horizon, and the boundary between them
/// is **the horizon at this altitude**, not the geometric horizontal.
#[test]
fn the_horizon_sits_in_the_middle_of_the_table_and_drops_with_height() {
    let mut previous = f64::INFINITY;
    for altitude in [2.0, 10_000.0, 100_000.0] {
        let r = BOTTOM + altitude;
        let (mu_v, _) = atmosphere::skyview_uv(BOTTOM, r, 0.0, 0.5);
        // The horizon from altitude `h` has
        // `cos(zenith) = -sqrt(r^2 - bottom^2)/r`.
        let expected = -(r * r - BOTTOM * BOTTOM).sqrt() / r;
        assert!(
            (mu_v - expected).abs() < 1.0e-9,
            "altitude {altitude}: {mu_v} against {expected}"
        );
        assert!(
            mu_v < previous,
            "the horizon did not drop at altitude {altitude}"
        );
        previous = mu_v;
    }
}
