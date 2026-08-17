//! Air on the CPU: the same physics as `shaders/sky.slang`, but in `f64` and
//! without a GPU (ROADMAP-ATMOSPHERE.md, S2).
//!
//! ## Why a second copy
//!
//! Rule 2 of stage S demands a **number** from every LUT, not "looks like
//! sky". That number has to come from somewhere, and it cannot come from the
//! shader itself: that would only check that the GPU can add. So a literal
//! twin lives here -- the same construction that has already paid for itself
//! twice in this project: `engine::cull` against `cull.slang`,
//! `Terrain::height_m` against `sample_height`.
//!
//! ## What the twin itself stands on
//!
//! The twin is numeric integration too, so it also needs pinning down;
//! otherwise two identical mistakes would agree and call themselves a check.
//! What pins it is the **closed form**: for a vertical ray the optical depth
//! through an exponential atmosphere is `beta*H*(exp(-h0/H) - exp(-h1/H))`,
//! and through the triangular ozone layer it is a trapezoid integral, closed
//! form as well. So the chain is:
//!
//! 1. closed form vs [`optical_depth`] -- unit test, no GPU;
//! 2. [`optical_depth`] vs the table on the GPU -- device test, over dozens of
//!    altitudes and angles (`engine/tests/atmosphere.rs`).
//!
//! No link checks itself.
//!
//! ## Geometry
//!
//! The same pair `(r, mu)` everywhere: `r` is the distance from the **centre
//! of the body**, `mu` the cosine of the angle between the ray direction and
//! the zenith (the direction away from the centre). This is the
//! parametrisation from the paper, and it is exactly the one in which the
//! transmittance table has its first column strictly vertical -- which is what
//! the closed form stands on.

use crate::scene::Atmosphere;

/// Width of the transmittance table -- the `mu` axis.
///
/// Must match `TRANSMITTANCE_WIDTH` in `shaders/sky.slang`; there is no
/// constant shared between Rust and Slang, so the `engine::tests::atmosphere`
/// test greps the shader file to compare them. The same trick as with `SIDE`
/// for patches (R6a).
pub const TRANSMITTANCE_WIDTH: u32 = 256;
/// Height of the transmittance table -- the `r` axis.
pub const TRANSMITTANCE_HEIGHT: u32 = 64;

/// How many steps the numeric integration takes **here**.
///
/// Four times more than in the shader (500 there), and the difference is
/// deliberate: two computations on the same grid would share their
/// discretisation error, i.e. agree even when both are wrong. Measured on the
/// worst ray of the table: 500 steps give 3.6e-5, 2048 give 2.1e-6, so the
/// oracle is more than an order of magnitude better than what it checks. An
/// oracle is allowed to be expensive -- it runs in a test, not in a frame.
pub const ORACLE_STEPS: usize = 2048;

/// The smallest extinction still safe to divide by.
///
/// Empty air gives zero in both numerator and denominator; `max` makes the
/// expression defined without changing the result -- at zero extinction
/// `1 - exp(0)` is zero too. The `max`-instead-of-branch shape came from the
/// shader, where it is forced: HLSL cannot index a vector with a variable, so
/// a loop over channels cannot exist there at all (`sky.slang`, the `X3511`
/// warning).
const TINY: f64 = 1.0e-30;

/// Density of the three air components at altitude `h` metres above the
/// surface: `[Rayleigh, Mie, ozone]`, dimensionless fraction of the
/// ground-level value.
///
/// Rayleigh and Mie are exponentials with their own scale heights. Ozone is a
/// triangle, as in the paper: it does not fall off with altitude at all but
/// has a layer at ~25 km, and that is why the zenith sky is blue rather than
/// violet.
pub fn density(air: &Atmosphere, h: f64) -> [f64; 3] {
    // There is no air below the surface. The clamp is not cosmetic: a ray that
    // dived underground would give `exp(+796)`, i.e. infinity, and that gives
    // NaN on the very first step. Caught on S3, in the shader.
    let h = h.max(0.0);
    let rayleigh = (-h / f64::from(air.rayleigh_height_m)).exp();
    let mie = (-h / f64::from(air.mie_height_m)).exp();
    let centre = f64::from(air.ozone_centre_m);
    let width = f64::from(air.ozone_width_m);
    let ozone = (1.0 - (h - centre).abs() / width).max(0.0);
    [rayleigh, mie, ozone]
}

/// Extinction coefficient at altitude `h`, 1/m, per RGB channel.
///
/// Extinction is scattering **plus** absorption: a ray loses a photon both
/// when it flies off sideways and when it is eaten. Rayleigh does not absorb
/// at all, Mie absorbs more than it scatters, ozone only absorbs.
pub fn extinction(air: &Atmosphere, h: f64) -> [f64; 3] {
    let [d_rayleigh, d_mie, d_ozone] = density(air, h);
    let mie = f64::from(air.mie_scattering) + f64::from(air.mie_absorption);
    let mut out = [0.0; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        *value = f64::from(air.rayleigh_scattering[channel]) * d_rayleigh
            + mie * d_mie
            + f64::from(air.ozone_absorption[channel]) * d_ozone;
    }
    out
}

/// `r^2 - bottom^2` -- the squared distance from the point to the grazing
/// point on the surface.
///
/// **This number, not the radius, is the natural variable of all the geometry
/// here**, and stage S tripped over that twice. Both distances -- to the
/// surface and to the top boundary -- are expressed through it without
/// subtracting large numbers, and the radius itself enters them only as a
/// factor. Whoever knows it more precisely (the table parametrisation, the
/// altitude above the surface) is the one who must pass it: in `f32` at
/// `r ~ 6.4e6` the difference of squares has a last-place unit of 4e6, so near
/// the grazing point rounding, not geometry, decides its sign.
pub fn rho_squared(r: f64, bottom: f64) -> f64 {
    (r * r - bottom * bottom).max(0.0)
}

/// The same for the top boundary: `top^2 - bottom^2`, written as a product.
///
/// A product, not a difference of squares: `(top - bottom)*(top + bottom)` is
/// a hundred kilometres times thirteen thousand, i.e. no cancellation at all.
pub fn shell_squared(air: &Atmosphere, bottom: f64) -> f64 {
    (air.top_m - bottom) * (air.top_m + bottom)
}

/// How many metres from the point `(r, mu)` to the top of the air.
///
/// `rho2` is [`rho_squared`] of this point, `shell2` is [`shell_squared`] of
/// the atmosphere. The root of the quadratic `|r*zenith + d*dir|^2 = top^2`,
/// rewritten through them: `d = -r*mu + sqrt(r^2*mu^2 + shell2 - rho2)`. The
/// second root is always negative when the point is inside the air, so there
/// is no choice to make here.
pub fn distance_to_top(r: f64, mu: f64, rho2: f64, shell2: f64) -> f64 {
    let discriminant = r * r * mu * mu + (shell2 - rho2);
    (-r * mu + discriminant.max(0.0).sqrt()).max(0.0)
}

/// How many metres to the surface, or `None` if the ray never meets it.
///
/// A ray pointing up (`mu >= 0`) never sees the surface -- and that has to be
/// checked separately, because the discriminant there can be positive too:
/// that is the second, "rear" intersection, which is not ahead.
pub fn distance_to_ground(r: f64, mu: f64, rho2: f64) -> Option<f64> {
    let discriminant = r * r * mu * mu - rho2;
    if mu >= 0.0 || discriminant < 0.0 {
        return None;
    }
    // `max(0)` is not cosmetic. A ray going down from the surface itself has
    // `rho^2 = 0`, and the difference `-r*mu - sqrt(r^2*mu^2)` comes out in
    // `f32` sometimes slightly positive, sometimes slightly negative. The
    // caller reads a negative one as "no surface ahead" and takes the ray
    // through the planet. Caught on S3: row 0 of the scattering table came out
    // three times brighter than the twin.
    Some((-r * mu - discriminant.sqrt()).max(0.0))
}

/// How many metres the ray `(r, mu)` travels through air: to the surface or to
/// the top boundary, whichever is nearer.
///
/// This function must not be used for rays of the **transmittance table**, and
/// that is not taste -- see [`optical_depth_to_top`].
pub fn span_in_air(air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> f64 {
    let rho2 = rho_squared(r, bottom);
    let top = distance_to_top(r, mu, rho2, shell_squared(air, bottom));
    match distance_to_ground(r, mu, rho2) {
        Some(ground) => ground.min(top),
        None => top,
    }
}

/// Optical depth over the segment `span` along the ray `(r, mu)`, per RGB
/// channel.
///
/// Midpoint rule, `steps` steps. Not Simpson: near the horizon the integrand
/// changes by orders of magnitude over one step, so the gain of a higher order
/// there is imaginary while the cost is real.
pub fn optical_depth(
    air: &Atmosphere,
    bottom: f64,
    r: f64,
    mu: f64,
    span: f64,
    steps: usize,
) -> [f64; 3] {
    let step = span / steps as f64;

    let mut out = [0.0; 3];
    for k in 0..steps {
        let d = (k as f64 + 0.5) * step;
        // Altitude of the step midpoint: law of cosines in the
        // centre-point-midpoint triangle.
        let h = (r * r + d * d + 2.0 * r * d * mu).max(0.0).sqrt() - bottom;
        let e = extinction(air, h);
        for (value, add) in out.iter_mut().zip(e.iter()) {
            *value += add * step;
        }
    }
    out
}

/// Optical depth from `(r, mu)` **to the top boundary**, ignoring the surface.
///
/// This is what lies in the transmittance table, and "ignoring the surface" is
/// a requirement here, not a simplification. The table parametrisation covers
/// exactly those directions that do reach the top boundary; the last column is
/// the ray tangent to the surface. Asking such a ray whether it meets the
/// surface is not allowed at all: the answer rests on the difference
/// `r^2 - bottom^2`, which is zero at the tangent point, and rounding decides
/// its sign. This once already cost a path ten times too short (found on S2,
/// in `f32` on the GPU -- there the same difference gives +-1.5 km).
pub fn optical_depth_to_top(
    air: &Atmosphere,
    bottom: f64,
    r: f64,
    mu: f64,
    steps: usize,
) -> [f64; 3] {
    let span = distance_to_top(r, mu, rho_squared(r, bottom), shell_squared(air, bottom));
    optical_depth(air, bottom, r, mu, span, steps)
}

/// Transmittance from `(r, mu)` to the top boundary -- `exp(-optical depth)`.
pub fn transmittance(air: &Atmosphere, bottom: f64, r: f64, mu: f64, steps: usize) -> [f64; 3] {
    let depth = optical_depth_to_top(air, bottom, r, mu, steps);
    [(-depth[0]).exp(), (-depth[1]).exp(), (-depth[2]).exp()]
}

/// Optical depth of a **vertical** ray going up -- in closed form.
///
/// This is what pins [`optical_depth`] down. For an exponential layer it is
/// `beta*H*(exp(-h0/H) - exp(-h1/H))`, for the triangular ozone layer it is a
/// difference of the triangle's antiderivatives, elementary as well.
///
/// Up only and vertical only: the spherical geometry at an angle has no closed
/// form at all (that is the Chapman function, and it is not elementary). One
/// direction is enough -- the transmittance table is built so that its first
/// column is exactly this ray, so the closed form covers all 64 altitudes.
pub fn vertical_optical_depth(air: &Atmosphere, bottom: f64, r: f64) -> [f64; 3] {
    let h0 = r - bottom;
    let h1 = air.top_m - bottom;

    let exponential = |scale: f64| scale * ((-h0 / scale).exp() - (-h1 / scale).exp());
    let rayleigh = exponential(f64::from(air.rayleigh_height_m));
    let mie = exponential(f64::from(air.mie_height_m));
    let ozone = triangle_integral(
        f64::from(air.ozone_centre_m),
        f64::from(air.ozone_width_m),
        h1,
    ) - triangle_integral(
        f64::from(air.ozone_centre_m),
        f64::from(air.ozone_width_m),
        h0,
    );

    let mie_extinction = f64::from(air.mie_scattering) + f64::from(air.mie_absorption);
    let mut out = [0.0; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        *value = f64::from(air.rayleigh_scattering[channel]) * rayleigh
            + mie_extinction * mie
            + f64::from(air.ozone_absorption[channel]) * ozone;
    }
    out
}

/// Antiderivative of the triangular ozone profile:
/// `integral from 0 to h of max(0, 1 - |z - centre| / width) dz`.
fn triangle_integral(centre: f64, width: f64, h: f64) -> f64 {
    if h <= centre - width {
        0.0
    } else if h <= centre {
        let t = h - (centre - width);
        t * t / (2.0 * width)
    } else if h <= centre + width {
        let s = h - centre;
        width / 2.0 + s - s * s / (2.0 * width)
    } else {
        width
    }
}

/// The point `(r, mu)` for texel coordinates of the transmittance table.
///
/// The parametrisation from the paper, and exactly one of its properties
/// matters here, the one the whole check of this step stands on: at `u = 0` it
/// gives `mu = 1`, i.e. a ray straight up. That is no coincidence -- `u`
/// measures the ray length from the shortest (vertical, `top - r`) to the
/// longest (tangent to the surface), and the shortest is the vertical.
pub fn uv_to_r_mu(air: &Atmosphere, bottom: f64, u: f64, v: f64) -> (f64, f64) {
    let top = air.top_m;
    // Length of the tangent to the surface from the top boundary -- the
    // natural unit of the "horizontal" size of the atmosphere.
    let h = (top * top - bottom * bottom).sqrt();
    let rho = h * v;
    let r = (rho * rho + bottom * bottom).sqrt();

    let d_min = top - r;
    let d_max = rho + h;
    let d = d_min + u * (d_max - d_min);
    let mu = if d == 0.0 {
        1.0
    } else {
        ((h * h - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
    };
    (r, mu)
}

/// The texture coordinate holding the unit value `u`.
///
/// The ends of the unit range land in the **centres of the edge texels**, not
/// on the edges of the texture. Without that, `u = 0` (the vertical) would not
/// be in the table at all, and bilinear sampling at the ends would reach past
/// the edge. The trick is standard (Bruneton), and it is precisely what makes
/// the first column of the table checkable by the closed form.
pub fn unit_to_texture(u: f64, size: u32) -> f64 {
    let n = f64::from(size);
    0.5 / n + u * (1.0 - 1.0 / n)
}

/// Inverse of [`uv_to_r_mu`]. Needed by whoever reads the table, not by
/// whoever writes it.
pub fn r_mu_to_uv(air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> (f64, f64) {
    let top = air.top_m;
    let h = (top * top - bottom * bottom).sqrt();
    let rho2 = rho_squared(r, bottom);
    let rho = rho2.sqrt();
    let d = distance_to_top(r, mu, rho2, h * h);
    let d_min = top - r;
    let d_max = rho + h;
    let u = if d_max > d_min {
        ((d - d_min) / (d_max - d_min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (u, (rho / h).clamp(0.0, 1.0))
}

/// Side of the multiple-scattering table (S3).
///
/// Must match `MULTISCATTER_SIZE` in `shaders/sky.slang`.
pub const MULTISCATTER_SIZE: u32 = 32;

/// How many directions the integration over the sphere visits -- 8x8.
///
/// A grid, not random directions: a table that cannot be reproduced cannot be
/// an oracle either. The CPU twin builds the same set, and that is the only
/// reason the two can be placed side by side at all.
pub const MULTISCATTER_DIRECTIONS: u32 = 64;

/// How many steps a ray takes within one direction.
pub const MULTISCATTER_STEPS: u32 = 20;

/// The point `(r, mu_s)` for unit coordinates of the scattering table.
///
/// The parametrisation here is simple -- linear on both axes -- and that is
/// not laziness: in the transmittance table the non-linearity existed for the
/// sake of the tangent ray, i.e. so that the sharp edge of the horizon would
/// not smear across a texel. Here there is no sharp edge at all: multiple
/// scattering is what is left after averaging over the whole sphere of
/// directions.
pub fn multiscatter_uv(air: &Atmosphere, bottom: f64, u: f64, v: f64) -> (f64, f64) {
    let mu_s = (u * 2.0 - 1.0).clamp(-1.0, 1.0);
    let r = bottom + v * (air.top_m - bottom);
    (r, mu_s)
}

/// The transmittance table in memory -- a mirror of the one on the GPU.
///
/// Needed because the multiple-scattering twin (S3) reads transmittance **a
/// million times**, and recomputing it by integration each time would make a
/// test nobody would ever run. The shader does exactly the same -- reads the
/// table bilinearly -- so the twin is in fact closer to it this way, not
/// further.
pub struct Table {
    pub width: u32,
    pub height: u32,
    values: Vec<[f64; 3]>,
}

impl Table {
    /// Build the transmittance table the same way `sky.slang` does.
    pub fn transmittance(air: &Atmosphere, bottom: f64, steps: usize) -> Table {
        let width = TRANSMITTANCE_WIDTH;
        let height = TRANSMITTANCE_HEIGHT;
        let mut values = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let u = f64::from(x) / f64::from(width - 1);
                let v = f64::from(y) / f64::from(height - 1);
                let (r, mu) = uv_to_r_mu(air, bottom, u, v);
                values.push(super::atmosphere::transmittance(air, bottom, r, mu, steps));
            }
        }
        Table {
            width,
            height,
            values,
        }
    }

    /// Build the multiple-scattering table -- the twin of `multiscatter_main`
    /// (S3).
    ///
    /// Only `psi`; `f` is not stored here, because only the convergence check
    /// reads it, and that one looks at the table on the GPU.
    pub fn multiscatter(
        air: &Atmosphere,
        bottom: f64,
        transmittance: &Table,
        albedo: [f64; 3],
    ) -> Table {
        let size = MULTISCATTER_SIZE;
        let mut values = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                let u = f64::from(x) / f64::from(size - 1);
                let v = f64::from(y) / f64::from(size - 1);
                let (r, mu_s) = multiscatter_uv(air, bottom, u, v);
                let (psi, _) = multiple_scattering(air, bottom, transmittance, r, mu_s, albedo);
                values.push(psi);
            }
        }
        Table {
            width: size,
            height: size,
            values,
        }
    }

    /// The value at **unit** coordinates -- bilinear, like `SampleLevel` in the
    /// shader.
    ///
    /// Unit, not texture coordinates: the parametrisation is the caller's
    /// business, and that is exactly why one table serves three different ones
    /// (S2, S3, S4).
    pub fn sample_unit(&self, u: f64, v: f64) -> [f64; 3] {
        // From the unit range into a texture coordinate, from there into a
        // texel index. `- 0.5` because texel `k` lives at coordinate
        // `(k + 0.5)/size`.
        let x = unit_to_texture(u, self.width) * f64::from(self.width) - 0.5;
        let y = unit_to_texture(v, self.height) * f64::from(self.height) - 0.5;
        self.bilinear(x, y)
    }

    /// Transmittance from `(r, mu)` to the top boundary.
    pub fn transmittance_at(&self, air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> [f64; 3] {
        let (u, v) = r_mu_to_uv(air, bottom, r, mu);
        self.sample_unit(u, v)
    }

    /// Multiple scattering at the point `(r, mu_s)`.
    pub fn multiscatter_at(&self, air: &Atmosphere, bottom: f64, r: f64, mu_s: f64) -> [f64; 3] {
        let u = (mu_s * 0.5 + 0.5).clamp(0.0, 1.0);
        let v = ((r - bottom) / (air.top_m - bottom)).clamp(0.0, 1.0);
        self.sample_unit(u, v)
    }

    fn bilinear(&self, x: f64, y: f64) -> [f64; 3] {
        let clamp_index = |value: f64, size: u32| -> (usize, usize, f64) {
            let floor = value.floor();
            let t = value - floor;
            let lo = (floor as i64).clamp(0, i64::from(size) - 1) as usize;
            let hi = (floor as i64 + 1).clamp(0, i64::from(size) - 1) as usize;
            (lo, hi, t.clamp(0.0, 1.0))
        };
        let (x0, x1, tx) = clamp_index(x, self.width);
        let (y0, y1, ty) = clamp_index(y, self.height);

        let at = |x: usize, y: usize| self.values[y * self.width as usize + x];
        let mut out = [0.0; 3];
        for (channel, value) in out.iter_mut().enumerate() {
            let top = at(x0, y0)[channel] * (1.0 - tx) + at(x1, y0)[channel] * tx;
            let bottom = at(x0, y1)[channel] * (1.0 - tx) + at(x1, y1)[channel] * tx;
            *value = top * (1.0 - ty) + bottom * ty;
        }
        out
    }
}

/// Direction number `k` of the uniform 8x8 grid on the sphere.
///
/// Written out the same way as in the shader, including the order: `k / 8`
/// goes along the azimuth, `k % 8` along the polar angle. The order by itself
/// does not affect the result (the sum is commutative), but a twin that walks
/// the sphere on a different grid is no longer a twin.
pub fn sphere_direction(k: u32) -> [f64; 3] {
    let i = 0.5 + f64::from(k / 8);
    let j = 0.5 + f64::from(k % 8);
    let theta = 2.0 * std::f64::consts::PI * i / 8.0;
    let phi = (1.0 - 2.0 * j / 8.0).acos();
    [phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos()]
}

/// Scattering (without absorption) at altitude `h`, 1/m per RGB channel.
///
/// Differs from [`extinction`] exactly in not counting what was eaten: ozone
/// does not scatter at all, Mie scatters less than it extinguishes. This is
/// the number that belongs in the scattering source -- a photon that was
/// absorbed does not fly into the sky.
pub fn scattering(air: &Atmosphere, h: f64) -> [f64; 3] {
    let [d_rayleigh, d_mie, _] = density(air, h);
    let mie = f64::from(air.mie_scattering) * d_mie;
    let mut out = [0.0; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        *value = f64::from(air.rayleigh_scattering[channel]) * d_rayleigh + mie;
    }
    out
}

/// Multiple scattering at the point `(r, mu_s)` -- the twin of
/// `multiscatter_main`.
///
/// Returns a pair: `psi` (the contribution of orders 2 and above, averaged
/// over the sphere of directions, per unit of solar irradiance) and `f`, the
/// fraction that a single scattering event returns back to the same point.
///
/// ## Where `psi = L2 / (1 - f)` comes from
///
/// The definition here is self-consistent, and it is worth reading before
/// editing.
///
/// `psi` is the **sphere-averaged radiance** at the point from orders 2 and
/// up. Then the isotropic scattering source at the point equals `sigma_s*psi`,
/// because `integral of L*(1/4pi) dw` is precisely that average.
///
/// `f` is computed like this: assume the medium glows uniformly with radiance
/// 1, and look at what average radiance returns to the point after **one**
/// scattering event. That is a linear operator, so the next order gives `f`
/// times the previous one, and the sum is a geometric series. Hence both
/// `1 / (1 - f)` and the requirement `f < 1`, which the test does check: if it
/// does not hold, the series does not converge, and "the energy grows" stops
/// being a metaphor.
///
/// ## What is deliberately absent here
///
/// **Nothing -- since T7h.** Reflection off the surface is here, and the
/// `albedo` argument is it: the mean albedo of the body, linear. Before T7h
/// there was a zero, and that was a decision rather than an omission -- the
/// surface colour did not exist in `crate::scene` at all back then, so any
/// number would have been made up. Now it comes from the tileset
/// (`Colour::mean`), and zero remains a legitimate value for a body whose
/// colour we do not know.
pub fn multiple_scattering(
    air: &Atmosphere,
    bottom: f64,
    table: &Table,
    r: f64,
    mu_s: f64,
    albedo: [f64; 3],
) -> ([f64; 3], [f64; 3]) {
    // Sun in the xz plane, point on the z axis: `up = (0, 0, 1)`.
    let sun = [(1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s];

    let shell2 = shell_squared(air, bottom);
    // `rho^2` of the point everything starts from. **Through the altitude, not
    // through a difference of squared radii**: at surface level the latter is
    // zero, and in `f32` rounding gives it its sign -- a downward ray then does
    // not stop at the ground but goes through the planet. Caught on S3: row 0
    // of the table differed from the twin by a factor of two, the rest by
    // 0.05%.
    let altitude = r - bottom;
    let rho2 = altitude * (2.0 * bottom + altitude);

    let mut second = [0.0; 3];
    let mut fraction = [0.0; 3];

    for k in 0..MULTISCATTER_DIRECTIONS {
        let w = sphere_direction(k);
        let mu = w[2];
        let mut span = distance_to_top(r, mu, rho2, shell2);
        if let Some(ground) = distance_to_ground(r, mu, rho2) {
            span = span.min(ground);
        }
        let step = span / f64::from(MULTISCATTER_STEPS);

        let mut throughput = [1.0; 3];
        for s in 0..MULTISCATTER_STEPS {
            let t = (f64::from(s) + 0.5) * step;
            // The sample point: `p + t*w` with `p = (0, 0, r)`.
            let point = [t * w[0], t * w[1], r + t * w[2]];
            // `rho^2` of the sample -- also without a difference of squares:
            // `|p + t*w|^2 - bottom^2 = rho^2 + 2*t*r*mu + t^2`.
            let rho2_here = (rho2 + 2.0 * t * r * mu + t * t).max(0.0);
            let radius = (rho2_here + bottom * bottom).max(0.0).sqrt();
            // The altitude comes from that same `rho^2`:
            // `rho^2 = (radius - bottom)(radius + bottom)`, and a sum of large
            // numbers has no cancellation.
            let h = rho2_here / (radius + bottom);
            let mu_s_here =
                (point[0] * sun[0] + point[1] * sun[1] + point[2] * sun[2]) / radius.max(1.0);

            // The planet's shadow: with the Sun below this point's horizon
            // there is no light at all, and this is where the night side comes
            // from.
            let lit = distance_to_ground(radius, mu_s_here, rho2_here).is_none();
            let to_sun = if lit {
                table.transmittance_at(air, bottom, radius, mu_s_here)
            } else {
                [0.0; 3]
            };

            let sigma_s = scattering(air, h);
            let sigma_e = extinction(air, h);

            for channel in 0..3 {
                let step_transmittance = (-sigma_e[channel] * step).exp();
                // The exact source integral over the step, not "the midpoint
                // value times the length": on the upper steps the ray dies out
                // within a single step, and the difference there is not
                // cosmetic.
                //
                // Empty air does not break the division: at zero extinction
                // `1 - exp(0)` is zero too, i.e. the contribution is zero,
                // while `TINY` in the denominator keeps the expression itself
                // defined. This is the same shape as in the shader, and there
                // it is forced -- HLSL cannot index a vector with a variable,
                // so a loop over channels cannot exist in it.
                let integrate =
                    |source: f64| source * (1.0 - step_transmittance) / sigma_e[channel].max(TINY);

                // Second scattering: what arrives at the point from direction
                // `w` is what scattered out of direct sunlight. The phase
                // function is uniform -- that is the paper's approximation.
                let uniform_phase = 1.0 / (4.0 * std::f64::consts::PI);
                second[channel] += throughput[channel]
                    * integrate(sigma_s[channel] * to_sun[channel] * uniform_phase);
                // The fraction: the same thing, but the medium glows with
                // radiance one from all sides, so the source is just `sigma_s`
                // (the integral of uniform radiance over the sphere with a
                // uniform phase gives one).
                fraction[channel] += throughput[channel] * integrate(sigma_s[channel]);

                throughput[channel] *= step_transmittance;
            }
        }

        // Reflection off the surface (T7h). A ray that fell to the ground
        // returns a Lambertian `albedo/pi * E`, and returns it **after** the
        // whole path, i.e. multiplied by whatever is left of it.
        //
        // For the fraction the source is the same, but the illumination is
        // isotropic with radiance one: `integral of cos/pi dw = 1`, so a
        // Lambertian surface gives back exactly `albedo`. Without this term
        // reflection would only be counted in the second order, and not in the
        // third and beyond.
        if let Some(ground) = distance_to_ground(r, mu, rho2) {
            let hit = [ground * w[0], ground * w[1], r + ground * w[2]];
            let length = (hit[0] * hit[0] + hit[1] * hit[1] + hit[2] * hit[2]).sqrt();
            let mu_s_ground = (hit[0] * sun[0] + hit[1] * sun[1] + hit[2] * sun[2]) / length;
            let lit = table.transmittance_at(air, bottom, bottom, mu_s_ground);
            for channel in 0..3 {
                if mu_s_ground > 0.0 {
                    second[channel] += throughput[channel] * albedo[channel] / std::f64::consts::PI
                        * lit[channel]
                        * mu_s_ground;
                }
                fraction[channel] += throughput[channel] * albedo[channel];
            }
        }
    }

    // Average over the sphere: `(4pi/N)` of solid angle per direction, divided
    // by the `4pi` of the averaging itself. What is left is `1/N`.
    let mut psi = [0.0; 3];
    for channel in 0..3 {
        second[channel] /= f64::from(MULTISCATTER_DIRECTIONS);
        fraction[channel] /= f64::from(MULTISCATTER_DIRECTIONS);
        psi[channel] = second[channel] / (1.0 - fraction[channel]).max(1.0e-6);
    }
    (psi, fraction)
}

/// Width of the sky-view table -- azimuth relative to the Sun (S4).
pub const SKYVIEW_WIDTH: u32 = 192;
/// Height of the sky-view table -- the view zenith angle.
pub const SKYVIEW_HEIGHT: u32 = 108;
/// How many steps a ray of the sky-view table takes.
pub const SKYVIEW_STEPS: u32 = 32;

/// The view direction for unit coordinates of the sky-view table.
///
/// Returns `(mu_v, cos_azimuth)`: the cosine of the view zenith angle and the
/// cosine of the azimuthal angle between the view and the Sun.
///
/// ## Why both axes are non-linear
///
/// **Along the zenith** -- because the horizon is sharp and the rest of the
/// sky is not. Half the table height goes to the hemisphere above the horizon,
/// half to the one below it, and within each half the step tightens towards
/// the horizon (square root). A linear scale would smear the band of sunset
/// across one texel out of a hundred and eight.
///
/// **Along the azimuth** -- because the Sun is small and the Mie phase
/// function is sharp: most of the colour variation happens within a few
/// degrees of the light source. The square compresses the side away from the
/// Sun and stretches the near one.
///
/// The boundary between the hemispheres is not the equator but the **horizon
/// at this altitude**: from ten kilometres up it is below the geometric
/// horizontal, and a table built at the equator would have its discontinuity
/// in a visible place.
pub fn skyview_uv(bottom: f64, r: f64, u: f64, v: f64) -> (f64, f64) {
    let rho2 = rho_squared(r, bottom);
    // Angle from the nadir to the horizon; the horizon's zenith angle is
    // `pi - beta`.
    let beta = (rho2.sqrt() / r).clamp(-1.0, 1.0).acos();
    let zenith_horizon = std::f64::consts::PI - beta;

    let zenith = if v < 0.5 {
        let c = 1.0 - 2.0 * v;
        zenith_horizon * (1.0 - c * c)
    } else {
        let c = 2.0 * v - 1.0;
        zenith_horizon + beta * c * c
    };
    // `1 - 2u^2` is the inverse of `u = sqrt((1 - cos)/2)`. At `u = 0` the view
    // is towards the Sun, at `u = 1` away from it.
    (zenith.cos(), 1.0 - 2.0 * u * u)
}

/// Inverse of [`skyview_uv`]: table coordinates for a view direction.
pub fn skyview_coords(bottom: f64, r: f64, mu_v: f64, cos_azimuth: f64) -> (f64, f64) {
    let rho2 = rho_squared(r, bottom);
    let beta = (rho2.sqrt() / r).clamp(-1.0, 1.0).acos();
    let zenith_horizon = std::f64::consts::PI - beta;
    let zenith = mu_v.clamp(-1.0, 1.0).acos();

    let v = if zenith <= zenith_horizon {
        let c = if zenith_horizon > 0.0 {
            1.0 - (1.0 - zenith / zenith_horizon).max(0.0).sqrt()
        } else {
            0.0
        };
        c * 0.5
    } else {
        let c = if beta > 0.0 {
            ((zenith - zenith_horizon) / beta).clamp(0.0, 1.0).sqrt()
        } else {
            0.0
        };
        0.5 + c * 0.5
    };
    let u = ((1.0 - cos_azimuth) * 0.5).max(0.0).sqrt();
    (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

/// The aerial-perspective volume (S5): [`AERIAL_XY`] columns across the screen,
/// [`AERIAL_Z`] slices along each ray.
///
/// Two constants rather than one cube, and D17 is why: the two axes answer
/// different questions -- how finely the volume follows the picture, and how
/// finely it follows the ray -- and a single number cannot be moved without
/// moving both. Duplicated in `shaders/sky.slang`, like every other table size
/// here.
pub const AERIAL_XY: u32 = 32;
pub const AERIAL_Z: u32 = 32;
/// How many march steps fall on one slice of the volume.
pub const AERIAL_SLICE_STEPS: u32 = 4;

/// Where the air in front of the camera at distance `r` begins and ends,
/// metres along the ray.
///
/// **The volume is stretched over this interval, not over "from the camera".**
/// From orbit the difference is decisive: the air lies a million metres ahead,
/// and a volume that starts at the camera spends thirty of its thirty-two
/// slices on vacuum. Measured on S5: from 1000 km up, the entire
/// hundred-kilometre layer fell into less than one slice of the volume.
///
/// - **The near edge** is the shortest distance to the shell: zero for a
///   camera inside, `r - top` outside.
/// - **The far one** is the longest ray that stays in air at all: from the
///   camera along the tangent to the surface and on to the top boundary. One
///   formula for both cases, and that is no coincidence: the second term is
///   that same tangent, only from the other end.
pub fn aerial_span(air: &Atmosphere, bottom: f64, r: f64) -> (f64, f64) {
    let near = (r - air.top_m).max(0.0);
    let far = rho_squared(r, bottom).sqrt() + shell_squared(air, bottom).sqrt();
    (near, far.max(near + 1.0))
}

/// The Rayleigh phase function: `3/(16pi)*(1 + cos^2(theta))`.
///
/// Symmetric forward and backward, and that is why the sky is bright behind
/// the observer too, not only around the Sun.
pub fn rayleigh_phase(cos_theta: f64) -> f64 {
    3.0 / (16.0 * std::f64::consts::PI) * (1.0 + cos_theta * cos_theta)
}

/// The Mie phase function -- Henyey-Greenstein with parameter `g`.
///
/// Sharply forward: `g = 0.8` means the aerosol scatters mostly along the
/// continuation of the ray. Hence both the halo around the Sun and the fact
/// that haze is seen against the light rather than with it.
pub fn mie_phase(cos_theta: f64, g: f64) -> f64 {
    let denominator = 1.0 + g * g - 2.0 * g * cos_theta;
    (1.0 - g * g) / (4.0 * std::f64::consts::PI * denominator.max(1.0e-6).powf(1.5))
}

/// The air together with both constant tables -- everything the sky is
/// computed from.
///
/// A struct rather than four separate arguments: without it [`Model::sky_view`]
/// would take eight, and none of them could be told apart by eye alone. It has
/// exactly as many fields as are read (CLAUDE.md), and they are owned, without
/// lifetimes -- a table costs half a megabyte and is built once per test.
pub struct Model {
    pub air: Atmosphere,
    pub bottom: f64,
    pub transmittance: Table,
    pub multiscatter: Table,
}

impl Model {
    /// Build both constant tables. `steps` is how many steps per ray in the
    /// transmittance table; 500 gives the same as the shader.
    pub fn build(air: &Atmosphere, bottom: f64, steps: usize, albedo: [f64; 3]) -> Model {
        let transmittance = Table::transmittance(air, bottom, steps);
        let multiscatter = Table::multiscatter(air, bottom, &transmittance, albedo);
        Model {
            air: *air,
            bottom,
            transmittance,
            multiscatter,
        }
    }

    /// Scattered light along a ray -- the twin of `skyview_main` (S4).
    ///
    /// The coordinate system is local: `up = (0, 0, 1)`, the Sun in the `xz`
    /// plane. The view is given by the pair `(mu_v, cos_azimuth)`, i.e. by
    /// exactly what lies on the table's axes -- and that is no loss: the
    /// physics depends only on `dot(view, sun)` and on the zenith angles of
    /// both, and the sign of the azimuth does not enter them.
    ///
    /// The ray stops at the surface and **adds nothing up to it**: the surface
    /// is drawn by the frame, not by the sky table. What is seen through the
    /// air is already aerial perspective, and that is a separate step (S5).
    pub fn sky_view(&self, r: f64, mu_s: f64, mu_v: f64, cos_azimuth: f64) -> [f64; 3] {
        let air = &self.air;
        let bottom = self.bottom;
        let transmittance = &self.transmittance;
        let multiscatter = &self.multiscatter;
        let sun = [(1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s];
        let sin_v = (1.0 - mu_v * mu_v).max(0.0).sqrt();
        let sin_azimuth = (1.0 - cos_azimuth * cos_azimuth).max(0.0).sqrt();
        let w = [sin_v * cos_azimuth, sin_v * sin_azimuth, mu_v];

        // The scattering angle is constant along the ray: both directions are
        // fixed.
        let cos_theta = w[0] * sun[0] + w[1] * sun[1] + w[2] * sun[2];
        let phase_r = rayleigh_phase(cos_theta);
        let phase_m = mie_phase(cos_theta, f64::from(air.mie_g));

        let rho2 = rho_squared(r, bottom);
        let mut span = distance_to_top(r, mu_v, rho2, shell_squared(air, bottom));
        if let Some(ground) = distance_to_ground(r, mu_v, rho2) {
            span = span.min(ground);
        }
        let step = span / f64::from(SKYVIEW_STEPS);

        let mut throughput = [1.0; 3];
        let mut light = [0.0; 3];
        for s in 0..SKYVIEW_STEPS {
            let t = (f64::from(s) + 0.5) * step;
            let point = [t * w[0], t * w[1], r + t * w[2]];
            let rho2_here = (rho2 + 2.0 * t * r * mu_v + t * t).max(0.0);
            let radius = (rho2_here + bottom * bottom).max(0.0).sqrt();
            let h = rho2_here / (radius + bottom);
            let mu_s_here =
                (point[0] * sun[0] + point[1] * sun[1] + point[2] * sun[2]) / radius.max(1.0);

            let lit = distance_to_ground(radius, mu_s_here, rho2_here).is_none();
            let to_sun = if lit {
                transmittance.transmittance_at(air, bottom, radius, mu_s_here)
            } else {
                [0.0; 3]
            };
            let psi = multiscatter.multiscatter_at(air, bottom, radius, mu_s_here);

            let [d_rayleigh, d_mie, _] = density(air, h);
            let sigma_e = extinction(air, h);

            for channel in 0..3 {
                let sigma_r = f64::from(air.rayleigh_scattering[channel]) * d_rayleigh;
                let sigma_m = f64::from(air.mie_scattering) * d_mie;
                // Direct light -- with each component's own phase function;
                // multiple scattering is already averaged over the sphere, so
                // it has no phase.
                let source = (sigma_r * phase_r + sigma_m * phase_m) * to_sun[channel]
                    + (sigma_r + sigma_m) * psi[channel];

                let step_transmittance = (-sigma_e[channel] * step).exp();
                light[channel] += throughput[channel] * source * (1.0 - step_transmittance)
                    / sigma_e[channel].max(TINY);
                throughput[channel] *= step_transmittance;
            }
        }
        light
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTTOM: f64 = 6_371_000.0;

    fn air() -> Atmosphere {
        Atmosphere::EARTH
    }

    /// Link 1 of the chain: numeric integration agrees with the closed form.
    ///
    /// The vertical ray is the only direction in which a closed form exists,
    /// and that is exactly why the table is built to contain it. Checked at
    /// every altitude of the layer rather than at a single point: an error in
    /// the ozone density is only visible where the ozone is.
    #[test]
    fn the_numeric_integral_matches_the_closed_form_going_straight_up() {
        let air = air();
        let thickness = air.top_m - BOTTOM;
        let mut worst: f64 = 0.0;
        for k in 0..=20 {
            let r = BOTTOM + thickness * f64::from(k) / 20.0;
            let numeric = optical_depth_to_top(&air, BOTTOM, r, 1.0, ORACLE_STEPS);
            let closed = vertical_optical_depth(&air, BOTTOM, r);
            for channel in 0..3 {
                // Relative error: near the top boundary both numbers are
                // small, and an absolute one would say nothing.
                let scale = closed[channel].abs().max(1.0e-12);
                worst = worst.max((numeric[channel] - closed[channel]).abs() / scale);
            }
        }
        assert!(
            worst < 1.0e-3,
            "worst relative error {worst}, should be the integration step"
        );
    }

    /// The same check, but for a point ABOVE the ozone layer.
    ///
    /// Separate, because here the triangle's antiderivative enters through
    /// both branches -- and an error in it right at the seam between them
    /// would go unnoticed in the test above.
    #[test]
    fn the_ozone_layer_integrates_through_its_own_peak() {
        let air = air();
        // 25 km is exactly the centre of the layer, i.e. the kink of the
        // profile.
        let numeric = optical_depth_to_top(&air, BOTTOM, BOTTOM + 25_000.0, 1.0, ORACLE_STEPS);
        let closed = vertical_optical_depth(&air, BOTTOM, BOTTOM + 25_000.0);
        for channel in 0..3 {
            let relative = (numeric[channel] - closed[channel]).abs() / closed[channel];
            assert!(relative < 1.0e-3, "channel {channel}: error {relative}");
        }
    }

    /// The first column of the table looks straight up. The whole S2 oracle
    /// stands on this.
    #[test]
    fn the_first_column_of_the_table_looks_straight_up() {
        let air = air();
        for k in 0..=8 {
            let v = f64::from(k) / 8.0;
            let (r, mu) = uv_to_r_mu(&air, BOTTOM, 0.0, v);
            assert!((mu - 1.0).abs() < 1.0e-9, "v = {v}: mu = {mu}");
            assert!(r >= BOTTOM - 1.0 && r <= air.top_m + 1.0, "r = {r}");
        }
    }

    /// The last column is tangent to the surface: a ray grazing the horizon.
    #[test]
    fn the_last_column_grazes_the_ground() {
        let air = air();
        for k in 1..=8 {
            let v = f64::from(k) / 8.0;
            let (r, mu) = uv_to_r_mu(&air, BOTTOM, 1.0, v);
            // A tangent from altitude r has `mu = -sqrt(r^2 - bottom^2)/r`.
            let expected = -(r * r - BOTTOM * BOTTOM).sqrt() / r;
            assert!(
                (mu - expected).abs() < 1.0e-9,
                "v = {v}: {mu} vs {expected}"
            );
        }
    }

    /// The forward and inverse transforms give back the same point.
    ///
    /// This is not a tautology: the inverse is computed by a different formula
    /// (through [`distance_to_top`]), and it is the one that will read the
    /// table in the frame.
    #[test]
    fn the_parametrisation_survives_a_round_trip() {
        let air = air();
        let mut worst: f64 = 0.0;
        for i in 0..16 {
            for j in 0..16 {
                let u = (f64::from(i) + 0.5) / 16.0;
                let v = (f64::from(j) + 0.5) / 16.0;
                let (r, mu) = uv_to_r_mu(&air, BOTTOM, u, v);
                let (u2, v2) = r_mu_to_uv(&air, BOTTOM, r, mu);
                worst = worst.max((u - u2).abs()).max((v - v2).abs());
            }
        }
        assert!(worst < 1.0e-6, "worst mismatch {worst}");
    }

    /// A ray pointing up never meets the surface, one pointing down does.
    #[test]
    fn only_a_ray_pointing_down_can_meet_the_ground() {
        let r = BOTTOM + 50_000.0;
        let rho2 = rho_squared(r, BOTTOM);
        assert!(distance_to_ground(r, 0.5, rho2).is_none());
        assert!(distance_to_ground(r, 0.0, rho2).is_none());
        // Straight down: the surface is exactly the altitude away.
        let down = distance_to_ground(r, -1.0, rho2).expect("downwards there is ground");
        assert!((down - 50_000.0).abs() < 1.0e-6, "{down}");
        // A grazing ray slightly below the tangent does reach the surface.
        let grazing = -rho2.sqrt() / r;
        assert!(distance_to_ground(r, grazing - 1.0e-6, rho2).is_some());
    }

    /// A ray leaving the surface downwards goes nowhere -- and that is a zero,
    /// not a "none".
    ///
    /// The smallest of this module's claims and the most expensive of them: in
    /// `f32` this very expression without the clamp came out negative, the
    /// caller read it as "no surface ahead", and the ray went through the
    /// planet. Row 0 of the scattering table came out three times brighter
    /// (S3).
    #[test]
    fn a_ray_leaving_the_surface_downwards_travels_nowhere() {
        for mu in [-1.0, -0.5, -0.001] {
            let d = distance_to_ground(BOTTOM, mu, 0.0).expect("the ground is right here");
            assert_eq!(d, 0.0, "mu = {mu}");
        }
    }

    /// `rho^2` through the altitude and through a difference of squares is the
    /// same number.
    ///
    /// In `f64` a difference of squares still works, so what is checked here
    /// is not the precision but that both formulas describe one quantity:
    /// that is what gives the shader the right to compute it the cheaper way.
    #[test]
    fn rho_squared_is_the_same_whether_it_comes_from_height_or_from_radii() {
        for altitude in [0.0, 10.0, 1_000.0, 100_000.0] {
            let r = BOTTOM + altitude;
            let by_height = altitude * (2.0 * BOTTOM + altitude);
            let by_radii = rho_squared(r, BOTTOM);
            let scale = by_height.max(1.0);
            assert!(
                (by_height - by_radii).abs() / scale < 1.0e-9,
                "altitude {altitude}: {by_height} vs {by_radii}"
            );
        }
        // The same for the shell.
        let air = air();
        let shell = shell_squared(&air, BOTTOM);
        assert!((shell - (air.top_m * air.top_m - BOTTOM * BOTTOM)).abs() / shell < 1.0e-9);
    }
}
