//! Where the aerial-perspective volume spends its slices, and what the
//! reconstruction between them costs (debt D17).
//!
//! A CPU twin of `aerial_main` in `f64`: it marches one ray finely, so it knows
//! the exact accumulated in-scattering `L(d)` at every distance, and then asks
//! what the volume would have answered -- the same march sampled at the slice
//! distances and read back by linear interpolation. The difference between the
//! two is the ring.
//!
//! No GPU, no frame: the question is about the slice layout, and the layout is
//! arithmetic.

use engine::atmosphere::{self, Model};
use engine::scene::Atmosphere;

const BOTTOM: f64 = 6_371_000.0;
const ALTITUDE: f64 = 400_000.0;

/// The Sun's cosine at the camera -- the same high sun the banding fixture uses.
const MU_S: f64 = 0.94;

fn main() {
    let air = Atmosphere::EARTH;
    let r = BOTTOM + ALTITUDE;
    let model = Model::build(&air, BOTTOM, 500, [0.0; 3]);
    let sun = [(1.0f64 - MU_S * MU_S).sqrt(), 0.0, MU_S];

    let (near, far) = atmosphere::aerial_span(&air, BOTTOM, r);
    println!(
        "camera at {:.0} km, span near {:.1} km  far {:.1} km",
        ALTITUDE / 1e3,
        near / 1e3,
        far / 1e3
    );
    println!(
        "slices {} , steps per slice {}",
        atmosphere::AERIAL_Z,
        atmosphere::AERIAL_SLICE_STEPS
    );

    // Where the slices fall, and how long they are.
    println!("\n-- slice layout --");
    println!("{:>5} {:>12} {:>12}", "slice", "distance km", "length km");
    let mut previous = 0.0;
    for s in 0..atmosphere::AERIAL_Z {
        let d = slice_distance(near, far, f64::from(s));
        if s < 20 || s % 8 == 0 {
            println!("{:>5} {:>12.2} {:>12.2}", s, d / 1e3, (d - previous) / 1e3);
        }
        previous = d;
    }

    // The accumulation profile of the nadir ray: where the in-scattering
    // actually happens along it.
    println!("\n-- nadir ray: where L accumulates --");
    let dir = [0.0, 0.0, -1.0];
    let ground = ground_distance(r, dir);
    let fine = march(&model, &air, r, dir, sun, ground, 4096);
    let total = fine.last().unwrap().1[1];
    println!("ground at {:.2} km", ground / 1e3);
    let s_ground = slice_of(near, far, ground);
    let slice_len = slice_distance(near, far, s_ground.floor())
        - slice_distance(near, far, (s_ground - 1.0).floor());
    // The slice length is in the list, and not as a literal: it is the whole
    // comparison. What one slice can resolve against what one slice must carry.
    for last_km in [1.0, 2.0, 5.0, 10.0, slice_len / 1e3, 50.0] {
        let share = 1.0 - value_at(&fine, ground - last_km * 1e3) / total;
        println!(
            "  the last {:>6.2} km before the ground carry {:>5.1}% of L",
            last_km,
            share * 100.0
        );
    }
    println!("  the ground sits at slice {s_ground:.2}");
    println!("  the slice there is {:.2} km long", slice_len / 1e3);

    // The reconstruction error across the frame: every pixel is a view angle,
    // and its distance is the distance to the ground along that ray.
    println!("\n-- reconstruction error vs angle from nadir --");
    // Two errors live here and they are not the same thing: the coarse march
    // itself (four steps over a slice), and the linear reconstruction between
    // the nodes. `quad %` is the first alone -- the node value against a fine
    // march to the same distance; `err %` is both together, i.e. what the pixel
    // gets.
    println!(
        "{:>8} {:>10} {:>8} {:>12} {:>12} {:>9} {:>9}",
        "angle deg", "ground km", "slice", "exact", "volume", "err %", "quad %"
    );
    let mut worst: f64 = 0.0;
    for i in 0..=70 {
        let theta: f64 = f64::from(i);
        let dir = [theta.to_radians().sin(), 0.0, -theta.to_radians().cos()];
        let d = ground_distance(r, dir);
        if d <= 0.0 {
            continue;
        }
        let exact = march(&model, &air, r, dir, sun, d, 2048).last().unwrap().1[1];
        let volume = read_volume(&air, &model, r, dir, sun, near, far, d);
        let err = (volume - exact) / exact.max(1e-30) * 100.0;
        // The node just below the pixel: the coarse march evaluated there,
        // against a fine march to the same distance.
        let node = slice_of(near, far, d).floor();
        let node_d = slice_distance(near, far, node).min(d);
        let node_coarse = read_volume(&air, &model, r, dir, sun, near, far, node_d);
        let node_exact = march(&model, &air, r, dir, sun, node_d, 2048)
            .last()
            .unwrap()
            .1[1];
        let quad = (node_coarse - node_exact) / node_exact.max(1e-30) * 100.0;
        worst = worst.max(err.abs());
        if i % 2 == 0 {
            println!(
                "{:>8.0} {:>10.2} {:>8.2} {:>12.4e} {:>12.4e} {:>9.2} {:>9.2}",
                theta,
                d / 1e3,
                slice_of(near, far, d),
                exact,
                volume,
                err,
                quad
            );
        }
    }
    println!("worst |err| over the frame: {worst:.2}%");

    // How much of the error is the layout and how much is the slice count.
    println!("\n-- worst |err| over the frame vs slice count, same layout --");
    for count in [32u32, 64, 128, 256, 512, 1024, 2048] {
        let mut worst: f64 = 0.0;
        for i in 0..=70 {
            let theta = f64::from(i);
            let dir = [theta.to_radians().sin(), 0.0, -theta.to_radians().cos()];
            let d = ground_distance(r, dir);
            let exact = march(&model, &air, r, dir, sun, d, 2048).last().unwrap().1[1];
            let volume = read_quadratic(&air, &model, r, dir, sun, near, far, d, count);
            worst = worst.max(((volume - exact) / exact * 100.0).abs());
        }
        println!("{count:>6} slices: {worst:>7.2}%");
    }

    // D17a: can the reader get tau without marching? The shader has to, and it
    // has only the transmittance table to do it with.
    println!("\n-- tau from two table reads vs a fine march --");
    // WARNING: the column that decides is `d slices`, not `err %`. Early on the
    //   ray tau is 1e-5, so a relative error there divides by nothing and reads
    //   -100% while the absolute miss is 1e-5. What the axis actually cares
    //   about is where the reader LANDS -- so the last column is the difference
    //   converted through `tau_to_z` into slices of a 64-slice volume, which is
    //   the unit the reconstruction is paid in.
    println!(
        "{:>8} {:>10} {:>12} {:>12} {:>9} {:>9}",
        "angle deg", "at km", "march", "table", "err %", "d slices"
    );
    // WARNING: the sample points are fractions of the AIR segment, not of the
    //   ray. Fractions of the ray put the first probe exactly on the entry into
    //   the shell, where tau is zero and a relative error is a division by
    //   nothing -- the first version of this table printed -100% there and said
    //   nothing at all. For the same reason the angles stop at the limb: from
    //   400 km the horizon is at 70.2 degrees, and a ray above it never touches
    //   air, so there is no tau to compare.
    let mut worst_tau: f64 = 0.0;
    for i in [0, 20, 40, 60, 68, 70] {
        let theta = f64::from(i);
        let dir = [theta.to_radians().sin(), 0.0, -theta.to_radians().cos()];
        let Some((entry, exit)) = air_segment(&air, r, dir) else {
            continue;
        };
        let profile = optical_profile(&air, r, dir, exit, 4096);
        for frac in [0.25, 0.5, 1.0] {
            let d = entry + (exit - entry) * frac;
            let marched = value_of(&profile, d / exit);
            let tabled = tau_from_table(&model, &air, r, dir, d);
            let err = (tabled - marched) / marched.max(1e-30) * 100.0;
            let slices =
                (tau_to_z(tabled, 0.1) - tau_to_z(marched, 0.1)) * f64::from(atmosphere::AERIAL_Z);
            worst_tau = worst_tau.max(slices.abs());
            println!(
                "{:>8.0} {:>10.2} {:>12.6} {:>12.6} {:>9.3} {:>9.4}",
                theta,
                d / 1e3,
                marched,
                tabled,
                err,
                slices
            );
        }
    }
    println!("worst miss on the axis: {worst_tau:.4} of a slice out of 64");

    // The same count, but the depth axis laid out in optical depth instead of
    // distance: nodes where the air is, not where the camera is.
    //
    // Measured twice: with tau from a march (what the axis is worth) and with
    // tau from the table (what the shader can actually afford). If the two
    // columns agree, D17a is proven and the axis is implementable.
    println!("\n-- worst |err| over the frame, depth axis in optical depth --");
    println!("tau0 is the half-way point of the axis: z = tau/(tau + tau0)");
    println!(
        "{:>6} {:>8} {:>12} {:>12}",
        "tau0", "slices", "tau marched", "tau from LUT"
    );
    for tau0 in [0.05f64, 0.1, 0.2, 0.4] {
        for count in [16u32, 32, 64] {
            let mut worst = [0.0f64; 2];
            for i in 0..=70 {
                let theta = f64::from(i);
                let dir = [theta.to_radians().sin(), 0.0, -theta.to_radians().cos()];
                let d = ground_distance(r, dir);
                let exact = march(&model, &air, r, dir, sun, d, 2048).last().unwrap().1[1];
                for (slot, from_table) in [false, true].into_iter().enumerate() {
                    let volume =
                        read_optical(&air, &model, r, dir, sun, d, count, tau0, from_table);
                    worst[slot] = worst[slot].max(((volume - exact) / exact * 100.0).abs());
                }
            }
            println!(
                "{tau0:>6} {count:>8} {:>11.2}% {:>11.2}%",
                worst[0], worst[1]
            );
        }
    }
}

/// The volume as it is today, but with a chosen slice count.
#[allow(clippy::too_many_arguments)]
fn read_quadratic(
    air: &Atmosphere,
    model: &Model,
    r: f64,
    dir: [f64; 3],
    sun: [f64; 3],
    near: f64,
    far: f64,
    distance: f64,
    count: u32,
) -> f64 {
    let limit = ground_distance(r, dir);
    let at = |slice: f64| {
        let w = slice / f64::from(count - 1);
        near + (far - near) * w * w
    };
    let mut nodes = Vec::with_capacity(count as usize);
    let (mut previous, mut light, mut throughput) = (0.0, 0.0, 1.0);
    for s in 0..count {
        let target = at(f64::from(s)).min(limit);
        let (l, t) = march_segment(model, air, r, dir, sun, previous, target, light, throughput);
        light = l;
        throughput = t;
        previous = target;
        nodes.push(light);
    }
    let w = ((distance - near) / (far - near)).clamp(0.0, 1.0).sqrt();
    lerp_nodes(&nodes, w * f64::from(count - 1))
}

/// The depth axis: optical depth squashed into the unit range.
///
/// A bijection rather than a division by a per-frame maximum, and that is the
/// whole point. Normalising by the thickest ray in the frame was tried first
/// and measured: `tau_ref` comes out 2.09 (the most slanted ray), which leaves
/// the nadir ray four nodes out of sixty-four and 12.4% of error -- the same
/// disease as normalising by the longest distance, in a new coordinate.
/// `tau/(tau + tau0)` instead gives every ray a share of the axis set by its
/// own thickness.
///
/// `tau0` is where the axis is half spent. Earth's vertical optical depth is
/// 0.13 in green, and the measured optimum sits at 0.1-0.2 -- i.e. the constant
/// is a property of the air, not a knob.
fn tau_to_z(tau: f64, tau0: f64) -> f64 {
    tau / (tau + tau0)
}

fn z_to_tau(z: f64, tau0: f64) -> f64 {
    let z = z.clamp(0.0, 0.999_999);
    tau0 * z / (1.0 - z)
}

/// The volume with its depth axis in **optical depth** instead of distance.
///
/// The reader can invert this without marching: `tau(d)` is the log of the
/// ratio of two transmittance-table reads, the camera's and the point's. The
/// probe does it by marching only because it is a probe.
#[allow(clippy::too_many_arguments)]
fn read_optical(
    air: &Atmosphere,
    model: &Model,
    r: f64,
    dir: [f64; 3],
    sun: [f64; 3],
    distance: f64,
    count: u32,
    tau0: f64,
    tau_from_lut: bool,
) -> f64 {
    let limit = ground_distance(r, dir);
    let profile = optical_profile(air, r, dir, limit, 4096);
    let tau_at = |d: f64| -> f64 {
        if tau_from_lut {
            return tau_from_table(model, air, r, dir, d);
        }
        let x = (d / limit * 4096.0).clamp(0.0, 4095.0);
        let i = x.floor() as usize;
        let f = x - i as f64;
        profile[i] * (1.0 - f) + profile[i + 1] * f
    };
    let distance_at = |tau: f64| -> f64 {
        match profile.iter().position(|&t| t >= tau) {
            Some(0) => 0.0,
            Some(i) => {
                let f = (tau - profile[i - 1]) / (profile[i] - profile[i - 1]).max(1e-30);
                (i as f64 - 1.0 + f) / 4096.0 * limit
            }
            None => limit,
        }
    };

    let mut nodes = Vec::with_capacity(count as usize);
    let (mut previous, mut light, mut throughput) = (0.0, 0.0, 1.0);
    for s in 0..count {
        let tau = z_to_tau(f64::from(s) / f64::from(count - 1), tau0);
        let target = distance_at(tau);
        let (l, t) = march_segment(model, air, r, dir, sun, previous, target, light, throughput);
        light = l;
        throughput = t;
        previous = target;
        nodes.push(light);
    }
    let z = tau_to_z(tau_at(distance), tau0).clamp(0.0, 1.0) * f64::from(count - 1);
    lerp_nodes(&nodes, z)
}

/// The optical depth from the camera to a point at `distance` -- from **two**
/// reads of the transmittance table, with no march at all (D17a).
///
/// This is what makes the optical-depth axis affordable in the shader: the
/// composition pass has to turn a pixel's distance into a depth coordinate, and
/// it cannot march to do it. `sample_transmittance` already exists there.
///
/// WARNING: the branch is on **whether the ray meets the ground**, not on the
///   sign of `mu`. The table holds the path to the TOP boundary, and that path
///   is only physical when it does not cross the planet. A limb ray leaves the
///   camera going down (`mu < 0`) and still misses the ground, and it belongs
///   in the first branch; reading it from the other end would ask the table for
///   a downgoing ray, which its parameterisation does not hold. This is
///   Bruneton's split, and it is the whole reason one table can serve both.
fn tau_from_table(model: &Model, air: &Atmosphere, r: f64, dir: [f64; 3], distance: f64) -> f64 {
    let mu = dir[2];
    let point = [distance * dir[0], distance * dir[1], r + distance * dir[2]];
    let r_d = (point[0] * point[0] + point[1] * point[1] + point[2] * point[2])
        .sqrt()
        .max(BOTTOM);
    let mu_d = (point[0] * dir[0] + point[1] * dir[1] + point[2] * dir[2]) / r_d;

    let hits_ground =
        atmosphere::distance_to_ground(r, mu, atmosphere::rho_squared(r, BOTTOM)).is_some();
    let table = &model.transmittance;
    let ratio = if hits_ground {
        // Read from the other end: both halves then go up.
        table.transmittance_at(air, BOTTOM, r_d, -mu_d)[1]
            / table.transmittance_at(air, BOTTOM, r, -mu)[1]
    } else {
        table.transmittance_at(air, BOTTOM, r, mu)[1]
            / table.transmittance_at(air, BOTTOM, r_d, mu_d)[1]
    };
    -ratio.clamp(1.0e-30, 1.0).ln()
}

/// Where the ray enters and leaves the air, metres along it. `None` if it never
/// meets the shell at all -- from 400 km that is every ray above the limb.
fn air_segment(air: &Atmosphere, r: f64, dir: [f64; 3]) -> Option<(f64, f64)> {
    let mu = dir[2];
    let rho2 = atmosphere::rho_squared(r, BOTTOM);
    let shell2 = atmosphere::shell_squared(air, BOTTOM);
    let discriminant = r * r * mu * mu + (shell2 - rho2);
    if discriminant < 0.0 || mu > 0.0 {
        return None;
    }
    let entry = (-r * mu - discriminant.sqrt()).max(0.0);
    let exit = match atmosphere::distance_to_ground(r, mu, rho2) {
        Some(ground) => ground,
        None => -r * mu + discriminant.sqrt(),
    };
    (exit > entry).then_some((entry, exit))
}

/// A value from a profile at a unit position along it.
fn value_of(profile: &[f64], unit: f64) -> f64 {
    let x = (unit * (profile.len() - 1) as f64).clamp(0.0, (profile.len() - 1) as f64);
    let i = (x.floor() as usize).min(profile.len() - 2);
    let f = x - i as f64;
    profile[i] * (1.0 - f) + profile[i + 1] * f
}

/// Cumulative optical depth along the ray, `steps + 1` samples over `[0, span]`.
fn optical_profile(air: &Atmosphere, r: f64, dir: [f64; 3], span: f64, steps: usize) -> Vec<f64> {
    let mu = dir[2];
    let rho2 = atmosphere::rho_squared(r, BOTTOM);
    let step = span / steps as f64;
    let mut tau = 0.0;
    let mut out = Vec::with_capacity(steps + 1);
    out.push(0.0);
    for s in 0..steps {
        let t = (s as f64 + 0.5) * step;
        let rho2_here = (rho2 + 2.0 * t * r * mu + t * t).max(0.0);
        let radius = (rho2_here + BOTTOM * BOTTOM).max(0.0).sqrt();
        let h = rho2_here / (radius + BOTTOM);
        tau += atmosphere::extinction(air, h)[1] * step;
        out.push(tau);
    }
    out
}

fn lerp_nodes(nodes: &[f64], position: f64) -> f64 {
    let lo = position.floor().clamp(0.0, nodes.len() as f64 - 1.0) as usize;
    let hi = (lo + 1).min(nodes.len() - 1);
    let f = (position - lo as f64).clamp(0.0, 1.0);
    nodes[lo] * (1.0 - f) + nodes[hi] * f
}

/// The distance of slice `s`, exactly as `aerial_distance` in `sky.slang`.
fn slice_distance(near: f64, far: f64, slice: f64) -> f64 {
    let w = slice / f64::from(atmosphere::AERIAL_Z - 1);
    near + (far - near) * w * w
}

/// The inverse: the (fractional) slice a distance is read at.
fn slice_of(near: f64, far: f64, distance: f64) -> f64 {
    let w = ((distance - near) / (far - near)).clamp(0.0, 1.0).sqrt();
    w * f64::from(atmosphere::AERIAL_Z - 1)
}

fn ground_distance(r: f64, dir: [f64; 3]) -> f64 {
    atmosphere::distance_to_ground(r, dir[2], atmosphere::rho_squared(r, BOTTOM)).unwrap_or(-1.0)
}

/// March one ray, returning `(distance, accumulated L)` at every step.
///
/// The source term is the one in `aerial_main`, in `f64`.
fn march(
    model: &Model,
    air: &Atmosphere,
    r: f64,
    dir: [f64; 3],
    sun: [f64; 3],
    span: f64,
    steps: usize,
) -> Vec<(f64, [f64; 3])> {
    let pos = [0.0, 0.0, r];
    let mu = dir[2];
    let rho2 = atmosphere::rho_squared(r, BOTTOM);
    let cos_theta = dir[0] * sun[0] + dir[1] * sun[1] + dir[2] * sun[2];
    let phase_r = atmosphere::rayleigh_phase(cos_theta);
    let phase_m = atmosphere::mie_phase(cos_theta, f64::from(air.mie_g));

    let step = span / steps as f64;
    let mut throughput = [1.0f64; 3];
    let mut light = [0.0f64; 3];
    let mut out = Vec::with_capacity(steps + 1);
    out.push((0.0, light));
    for s in 0..steps {
        let t = (s as f64 + 0.5) * step;
        let point = [
            pos[0] + t * dir[0],
            pos[1] + t * dir[1],
            pos[2] + t * dir[2],
        ];
        let rho2_here = (rho2 + 2.0 * t * r * mu + t * t).max(0.0);
        let radius = (rho2_here + BOTTOM * BOTTOM).max(0.0).sqrt();
        let h = rho2_here / (radius + BOTTOM);
        let mu_s = (point[0] * sun[0] + point[1] * sun[1] + point[2] * sun[2]) / radius.max(1.0);

        let lit = atmosphere::distance_to_ground(radius, mu_s, rho2_here).is_none();
        let to_sun = if lit {
            model
                .transmittance
                .transmittance_at(air, BOTTOM, radius, mu_s)
        } else {
            [0.0; 3]
        };
        let psi = model
            .multiscatter
            .multiscatter_at(air, BOTTOM, radius, mu_s);

        let d = atmosphere::density(air, h);
        let sigma_e = atmosphere::extinction(air, h);
        for c in 0..3 {
            let sigma_r = f64::from(air.rayleigh_scattering[c]) * d[0];
            let sigma_m = f64::from(air.mie_scattering) * d[1];
            let source =
                (sigma_r * phase_r + sigma_m * phase_m) * to_sun[c] + (sigma_r + sigma_m) * psi[c];
            let st = (-sigma_e[c] * step).exp();
            light[c] += throughput[c] * source * (1.0 - st) / sigma_e[c].max(1e-30);
            throughput[c] *= st;
        }
        out.push(((s as f64 + 1.0) * step, light));
    }
    out
}

/// The accumulated value at a distance, from a finely marched profile.
fn value_at(profile: &[(f64, [f64; 3])], distance: f64) -> f64 {
    let last = profile.last().unwrap().0;
    let x = (distance / last * (profile.len() - 1) as f64).clamp(0.0, (profile.len() - 1) as f64);
    let i = (x.floor() as usize).min(profile.len() - 2);
    let f = x - i as f64;
    profile[i].1[1] * (1.0 - f) + profile[i + 1].1[1] * f
}

/// What the volume answers at `distance` -- the column rebuilt for this exact
/// ray, so that the screen axis is out of the picture and only the depth axis
/// is measured.
#[allow(clippy::too_many_arguments)]
fn read_volume(
    air: &Atmosphere,
    model: &Model,
    r: f64,
    dir: [f64; 3],
    sun: [f64; 3],
    near: f64,
    far: f64,
    distance: f64,
) -> f64 {
    let limit = ground_distance(r, dir);
    // Build the column: the accumulated value at every slice node.
    let mut nodes = Vec::with_capacity(atmosphere::AERIAL_Z as usize);
    let mut previous = 0.0;
    let mut light = 0.0;
    let mut throughput = 1.0;
    for s in 0..atmosphere::AERIAL_Z {
        let target = slice_distance(near, far, f64::from(s)).min(limit);
        let (l, t) = march_segment(model, air, r, dir, sun, previous, target, light, throughput);
        light = l;
        throughput = t;
        previous = target;
        nodes.push(light);
    }
    // Read it back linearly, as the sampler does.
    let s = slice_of(near, far, distance);
    let lo = s.floor().clamp(0.0, f64::from(atmosphere::AERIAL_Z - 1)) as usize;
    let hi = (lo + 1).min(atmosphere::AERIAL_Z as usize - 1);
    let f = s - lo as f64;
    nodes[lo] * (1.0 - f) + nodes[hi] * f
}

/// One slice's worth of march, continuing an accumulation. Green channel only:
/// the ring is a shape, and three copies of it say the same thing.
#[allow(clippy::too_many_arguments)]
fn march_segment(
    model: &Model,
    air: &Atmosphere,
    r: f64,
    dir: [f64; 3],
    sun: [f64; 3],
    from: f64,
    to: f64,
    mut light: f64,
    mut throughput: f64,
) -> (f64, f64) {
    let pos = [0.0, 0.0, r];
    let mu = dir[2];
    let rho2 = atmosphere::rho_squared(r, BOTTOM);
    let cos_theta = dir[0] * sun[0] + dir[1] * sun[1] + dir[2] * sun[2];
    let phase_r = atmosphere::rayleigh_phase(cos_theta);
    let phase_m = atmosphere::mie_phase(cos_theta, f64::from(air.mie_g));
    let step = (to - from).max(0.0) / f64::from(atmosphere::AERIAL_SLICE_STEPS);

    for k in 0..atmosphere::AERIAL_SLICE_STEPS {
        let t = from + (f64::from(k) + 0.5) * step;
        let point = [
            pos[0] + t * dir[0],
            pos[1] + t * dir[1],
            pos[2] + t * dir[2],
        ];
        let rho2_here = (rho2 + 2.0 * t * r * mu + t * t).max(0.0);
        let radius = (rho2_here + BOTTOM * BOTTOM).max(0.0).sqrt();
        let h = rho2_here / (radius + BOTTOM);
        let mu_s = (point[0] * sun[0] + point[1] * sun[1] + point[2] * sun[2]) / radius.max(1.0);

        let lit = atmosphere::distance_to_ground(radius, mu_s, rho2_here).is_none();
        let to_sun = if lit {
            model
                .transmittance
                .transmittance_at(air, BOTTOM, radius, mu_s)[1]
        } else {
            0.0
        };
        let psi = model
            .multiscatter
            .multiscatter_at(air, BOTTOM, radius, mu_s)[1];

        let d = atmosphere::density(air, h);
        let sigma_e = atmosphere::extinction(air, h)[1];
        let sigma_r = f64::from(air.rayleigh_scattering[1]) * d[0];
        let sigma_m = f64::from(air.mie_scattering) * d[1];
        let source = (sigma_r * phase_r + sigma_m * phase_m) * to_sun + (sigma_r + sigma_m) * psi;
        let st = (-sigma_e * step).exp();
        light += throughput * source * (1.0 - st) / sigma_e.max(1e-30);
        throughput *= st;
    }
    (light, throughput)
}
