// Air after the Hillaire 2020 model (PROJECT.md §7,
// ROADMAP-ATMOSPHERE.md).
//
// A verbatim twin of `engine::atmosphere`, exactly as `cull.slang` is a twin
// of `engine::cull`. Both must be edited: the step's oracle is that the
// numbers on both sides agree, and once they diverge they stop checking each
// other.
//
// The units are **metres and 1/m**, as everywhere else in the engine. The
// paper works in kilometres, and that is precisely the conversion where three
// orders are most easily lost in silence (S1).

// The air parameters -- the same as `engine::scene::Atmosphere`, packed into
// four vectors. Packing rather than field by field: a std140 constant buffer
// would align every `float3` to 16 bytes anyway, so dense packing hides
// nothing here and makes the layout obvious.
struct AirParams
{
    // rgb -- Rayleigh scattering at the surface, 1/m; w -- its scale height, m.
    float4 rayleigh;
    // x -- Mie scattering, y -- Mie absorption (both 1/m), z -- scale height,
    // w -- phase function asymmetry.
    float4 mie;
    // rgb -- ozone absorption at the peak, 1/m; w -- unused.
    float4 ozone;
    // x -- centre of the ozone layer, y -- its half width (both m),
    // z -- radius of the body surface, w -- radius of the top of the air.
    float4 shape;
    // rgb -- mean albedo of the surface under this sky, linear (T7h);
    // w -- spare. Zero means "the surface is unknown", and the pass then
    // behaves as before T7h: the sky gets nothing reflected from below.
    float4 ground;
};

// ## The bind groups, and why there are two
//
// Group 0 is what the pass **reads**; group 1 is what it **writes**. The
// split is not cosmetic: the transmittance table is written by one pass and
// read by all the following ones, and a single bind group holding the same
// texture for both writing and reading is forbidden -- wgpu sees a race in it
// regardless of what the shader does. So the transmittance pass gets a group
// 0 without the table itself (a layout in which those bindings simply are
// not), and the rest get the full one.
//
// The slots of group 1 do not overlap: each pass has its own. Each pipeline's
// layout declares exactly its own slot, and that is allowed -- a layout is
// required to cover what the entry point reads, not everything in the
// module.
[[vk::binding(0, 0)]]
ConstantBuffer<AirParams> air;

// One sampler for all the tables: bilinear filtering with clamping at the
// edges. Clamping rather than repeat -- which is why the ends of the unit
// range sit in the centres of the outermost texels: otherwise half of the
// outermost texel would be read from the clamped edge rather than from the
// table.
[[vk::binding(1, 0)]]
SamplerState lut_sampler;

// The transmittance table: 256x64, rgba16float (S2).
//
// The sizes are duplicated in
// `engine::atmosphere::TRANSMITTANCE_WIDTH/HEIGHT`, and there is no constant
// shared between Rust and Slang -- a test greps this file, as it already does
// for the patches' `SIDE`.
static const uint TRANSMITTANCE_WIDTH = 256u;
static const uint TRANSMITTANCE_HEIGHT = 64u;

// How many steps the integration takes HERE.
//
// Five hundred, and the number is measured rather than taken from the paper
// (which says forty). Forty is enough everywhere except rays leaving the
// ground at a shallow angle: there a 560 km path is split into 14 km steps
// against a scale height of 8 km, and the midpoint rule is off by 5e-3 --
// eight times the storage step of half-float. Five hundred give 3.6e-5, i.e.
// they run into the format rather than the arithmetic. This is paid once per
// set of air parameters, not once per frame, so anything cheaper would only
// be wrong.
static const uint TRANSMITTANCE_STEPS = 500u;

// The multiple-scattering table: 32x32 (S3).
static const uint MULTISCATTER_SIZE = 32u;
// How many directions the integration over the sphere visits -- an 8x8 grid.
static const uint MULTISCATTER_DIRECTIONS = 64u;
// How many steps a ray takes inside one direction.
static const uint MULTISCATTER_STEPS = 20u;

// The sky-view table: 192x108 (S4).
static const uint SKYVIEW_WIDTH = 192u;
static const uint SKYVIEW_HEIGHT = 108u;
// How many steps a sky-view ray takes.
static const uint SKYVIEW_STEPS = 32u;

static const float PI = 3.14159265358979;

// WARNING: **no indexing a vector by a variable and no loops over channels.**
//
// HLSL cannot index a `float3` by a non-constant index at all -- the compiler
// is obliged to unroll such a loop, and inside the march, whose body is large
// already, FXC refuses: `error X3511: forced to unroll loop, but unrolling
// failed`. Caught in CI on WARP (D3D12), where Vulkan and Metal stayed
// silent.
//
// So all per-channel arithmetic is vector arithmetic. Instead of
// `if (sigma_e[c] <= 0) continue;` there is a division by `max(sigma_e,
// TINY)`: where the extinction is zero, `1 - exp(0)` is zero too, so the
// contribution is zero and the division is defined. Same result, and nothing
// to unroll.
static const float TINY = 1.0e-30;

// The transmittance table for reading -- for every pass but the one that
// writes it.
[[vk::binding(2, 0)]]
Texture2D<float4> transmittance_lut;

// The multiple-scattering table for reading.
[[vk::binding(3, 0)]]
Texture2D<float4> multiscatter_lut;

// What changes **every frame**: where the camera is and where the Sun is.
//
// A separate buffer from `air`, and that is not a detail: the air parameters
// live as long as the body does, these live one frame. One struct for both
// would mean that moving the camera looks like changing the atmosphere, i.e.
// recomputing tables nobody asked for (rule 5 of stage S).
struct ViewParams
{
    // x -- camera distance from the body centre, y -- cosine of the Sun's
    // zenith angle at the camera, z -- exposure (S4b), w -- the FAR edge of the
    // aerial perspective volume, metres along the ray (S5).
    float4 view;
    // xyz -- the camera relative to the BODY CENTRE, world axes. The
    // subtraction is done on the CPU in `double`; what is left here is a number
    // of order 6e6, in which `f32` gives half a metre -- sixty parts per
    // million of an eight-kilometre scale height. w -- the NEAR edge of the
    // aerial perspective volume (S5).
    float4 eye;
    // xyz -- direction TO the Sun, world axes.
    float4 sun;
    // The screen axes in world coordinates. `w` carries the tangents of the
    // view half-angles: `right.w = tan(fovy/2)*aspect`, `screen_up.w =
    // tan(fovy/2)`. Together they give the pixel ray -- no inverse matrix is
    // needed for it.
    float4 right;
    float4 screen_up;
    float4 forward;
};

[[vk::binding(4, 0)]]
ConstantBuffer<ViewParams> frame;

// `WTexture2D` (write only) rather than `RWTexture2D`: the latter compiles to
// `texture_storage_2d<..., read_write>`, and WebGPU allows read-write only for
// single-channel 32-bit formats. The format is declared right here, because
// WGSL requires it in the type -- it cannot be supplied from the Rust side.
[[vk::binding(0, 1)]]
[format("rgba16f")]
WTexture2D<float4> transmittance_out;

[[vk::binding(1, 1)]]
[format("rgba16f")]
WTexture2D<float4> multiscatter_out;

[[vk::binding(2, 1)]]
[format("rgba16f")]
WTexture2D<float4> skyview_out;

// The aerial perspective volume (S5): AERIAL_XY columns across the screen,
// AERIAL_Z slices along each ray.
//
// Two constants rather than one cube, because the two axes answer different
// questions and D17 had to ask them separately: the screen axis decides how
// finely the volume follows the picture, the depth axis how finely it follows
// the ray. A single number cannot be moved without moving both.
static const uint AERIAL_XY = 32u;
static const uint AERIAL_Z = 64u;
// How many march steps fall on one slice of the volume.
static const uint AERIAL_SLICE_STEPS = 4u;

// Two textures rather than one: the composition is `background*T + L`, i.e.
// **a multiply and an add per channel**. Transmittance collapsed to a single
// number would make a sunset through the air grey instead of red -- exactly
// what S4b just measured and what is worth keeping.
[[vk::binding(3, 1)]]
[format("rgba16f")]
WTexture3D<float4> aerial_inscatter_out;

[[vk::binding(4, 1)]]
[format("rgba16f")]
WTexture3D<float4> aerial_transmittance_out;

[[vk::binding(6, 0)]]
Texture3D<float4> aerial_inscatter_lut;

[[vk::binding(7, 0)]]
Texture3D<float4> aerial_transmittance_lut;

// The frame's depth as a texture -- to know how far to shade the air.
//
// The composition pass has no depth buffer of its own at all (`depth_stencil:
// None`), which is why this texture can be read: the same texture as both a
// target and a resource would be a race.
[[vk::binding(8, 0)]]
Texture2D<float> depth_texture;

// What tells one depth range from another (R4a).
//
// `z_ndc = -A + B/z` for both of the engine's projections: the infinite one
// has `A = 0`, `B = near`, the finite one `A = near/(far-near)`,
// `B = near*far/(far-near)`. Hence `z = B/(z_ndc + A)`, and a zero in `z_ndc`
// means "nothing was drawn here in this range".
struct PassParams
{
    float4 depth;  // x = A, y = B, zw -- padding
};

[[vk::binding(9, 0)]]
ConstantBuffer<PassParams> range;

// The sky-view table for reading -- for the fullscreen pass (S4b). A separate
// slot from the one it is written to: one texture for both writing and reading
// in one group is a race as far as wgpu is concerned, regardless of what the
// shader does.
[[vk::binding(5, 0)]]
Texture2D<float4> skyview_lut;

// The density of the three components at height `h` above the surface -- the
// twin of `atmosphere::density`.
//
// WARNING: the shape of the ozone layer lives in `shape`, NOT in `ozone`:
//   `ozone` holds the absorption coefficients themselves. Mixing them up is
//   easy -- both vectors are named for ozone -- and it has cost once already:
//   the layer came out two micrometres wide, i.e. there was no ozone at all.
//   That shows only on a shallow ray in the stratosphere, where ozone works;
//   at the zenith the difference drowns in the error.
float3 density(float h)
{
    // There is no air below the surface, and the clamp is not cosmetics here:
    // a ray that dived 6371 km underground would give `exp(+796)`, i.e.
    // infinity, and infinity times a zero step gives NaN -- and the whole table
    // comes out of it. Caught at S3, on the surface-level texels.
    h = max(h, 0.0);
    float rayleigh = exp(-h / air.rayleigh.w);
    float mie = exp(-h / air.mie.z);
    float ozone = max(0.0, 1.0 - abs(h - air.shape.x) / air.shape.y);
    return float3(rayleigh, mie, ozone);
}

// Extinction at height `h`, 1/m per RGB channel -- the twin of
// `atmosphere::extinction`.
float3 extinction(float h)
{
    float3 d = density(h);
    float mie = air.mie.x + air.mie.y;
    return air.rayleigh.rgb * d.x + float3(mie, mie, mie) * d.y + air.ozone.rgb * d.z;
}

// WARNING: **the variable here is `rho2 = r^2 - bottom^2`, not the radius**,
//   and stage S tripped over it twice. Both distances are expressed through it
//   with no subtraction of large numbers; whoever knows it more precisely (the
//   table parameterisation, the height above the surface) must pass it in. In
//   `f32` at `r ~ 6.4e6` the last-place unit of the square is 4e6, so near the
//   surface and near a tangent the sign of that difference is decided by
//   rounding rather than by geometry.
//
// `rho2` is a point's `r^2 - bottom^2`, `shell2` is the atmosphere's
// `top^2 - bottom^2`.

// How many metres to the top boundary -- the twin of
// `atmosphere::distance_to_top`.
float distance_to_top(float r, float mu, float rho2, float shell2)
{
    float discriminant = r * r * mu * mu + (shell2 - rho2);
    return max(0.0, -r * mu + sqrt(max(0.0, discriminant)));
}

// How many metres to the surface; negative means there is no surface ahead.
// The twin of `atmosphere::distance_to_ground`, which returns an `Option`
// there.
float distance_to_ground(float r, float mu, float rho2)
{
    float discriminant = r * r * mu * mu - rho2;
    if (mu >= 0.0 || discriminant < 0.0) { return -1.0; }
    // `max(0)` is not cosmetics. A ray going down from the surface itself has
    // `rho2 = 0`, and the difference `-r*mu - sqrt(r^2 mu^2)` comes out in
    // `f32` sometimes slightly positive and sometimes slightly negative. The
    // caller reads a negative one as "no surface ahead" and drives the ray
    // through the planet -- with the shadow computed from inside it. Caught at
    // S3: row 0 of the table came out three times brighter.
    return max(0.0, -r * mu - sqrt(discriminant));
}

// The optical depth over the segment `span` along `(r, mu)` -- the twin of
// `atmosphere::optical_depth`, midpoint rule.
//
// WARNING: the segment arrives as a **parameter** rather than being computed
//   here from `(r, mu)`, and that is not style. The end of a ray near a
//   tangent to the surface rests on the difference `r^2 - bottom^2`, which at
//   the tangent itself is zero; in `f32` at `r ~ 6.4e6` the last-place unit of
//   the square is 4e6, so the sign of that difference is decided by rounding
//   rather than by geometry. At S2 that cost a path cut tenfold: the outermost
//   column of the table diverged from the oracle by 0.21 instead of 1e-3.
//   Whoever supplies the segment almost always knows it more precisely -- the
//   table parameterisation, for one, gives it as `rho + H`, with no subtraction
//   at all.
float3 optical_depth(float r, float mu, float span, uint steps)
{
    float bottom = air.shape.z;
    float step = span / float(steps);
    float3 sum = float3(0.0, 0.0, 0.0);
    for (uint k = 0u; k < steps; ++k)
    {
        float d = (float(k) + 0.5) * step;
        float h = sqrt(max(0.0, r * r + d * d + 2.0 * r * d * mu)) - bottom;
        sum += extinction(h) * step;
    }
    return sum;
}

// The point `(r, mu)` from texel coordinates -- the twin of
// `atmosphere::uv_to_r_mu`.
//
// One property matters: at `u = 0` it gives `mu = 1`, i.e. a ray straight up,
// for which a closed form exists. The whole S2 oracle rests on it.
//
// The third output is the distance to the top boundary itself: the
// parameterisation already knows it (it is `d`), and returning it is cheaper
// and more precise than recovering it from `(r, mu)` -- see the warning in
// `optical_depth`.
void uv_to_r_mu(float2 uv, out float r, out float mu, out float d)
{
    float bottom = air.shape.z;
    float top = air.shape.w;

    float h = sqrt(max(0.0, top * top - bottom * bottom));
    float rho = h * uv.y;
    r = sqrt(rho * rho + bottom * bottom);

    float d_min = top - r;
    float d_max = rho + h;
    d = d_min + uv.x * (d_max - d_min);
    mu = d == 0.0 ? 1.0 : clamp((h * h - rho * rho - d * d) / (2.0 * r * d), -1.0, 1.0);
}

// The transmittance table: a texel per `(r, mu)` pair, the value being
// `exp(-tau)`.
//
// Computed once per set of air parameters rather than once per frame:
// transmittance depends on neither the camera nor the Sun
// (ROADMAP-ATMOSPHERE.md, the table in rule 5).
[shader("compute")]
[numthreads(8, 8, 1)]
void transmittance_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= TRANSMITTANCE_WIDTH || id.y >= TRANSMITTANCE_HEIGHT) { return; }

    // The ends of the range sit in the centres of the outermost texels rather
    // than outside the texture.
    //
    // So texel 0 carries exactly `u = 0`, i.e. exactly `mu = 1` -- the
    // vertical, the one direction with a closed form. Dividing by the width
    // instead of `width - 1` would mean the vertical is not in the table at
    // all and the nearest texel to it has `mu = 0.98`: there would be nothing
    // left to check the table against a formula with. The reader does the
    // inverse transform (`atmosphere::unit_to_texture`), which is why bilinear
    // sampling at the ends does not reach past the edge.
    float2 uv = float2(float(id.x) / float(TRANSMITTANCE_WIDTH - 1u),
                       float(id.y) / float(TRANSMITTANCE_HEIGHT - 1u));
    float r;
    float mu;
    float span;
    uv_to_r_mu(uv, r, mu, span);

    float3 depth = optical_depth(r, mu, span, TRANSMITTANCE_STEPS);
    transmittance_out.Store(id.xy, float4(exp(-depth), 1.0));
}

// ---------------------------------------------------------------------------
// Multiple scattering (S3)
// ---------------------------------------------------------------------------

// The texture coordinate holding the unit value `u` -- the twin of
// `atmosphere::unit_to_texture`. The ends of the range sit in the centres of
// the outermost texels; without that, bilinear sampling at the edge would
// read a clamped texel instead of the table.
float unit_to_texture(float u, uint size)
{
    float n = float(size);
    return 0.5 / n + u * (1.0 - 1.0 / n);
}

// Unit coordinates into the transmittance table from `(r, mu)` -- the twin of
// `atmosphere::r_mu_to_uv`.
float2 r_mu_to_uv(float r, float mu)
{
    float bottom = air.shape.z;
    float top = air.shape.w;

    float h = sqrt(max(0.0, top * top - bottom * bottom));
    float rho = sqrt(max(0.0, r * r - bottom * bottom));
    float d = distance_to_top(r, mu, rho * rho, h * h);
    float d_min = top - r;
    float d_max = rho + h;
    float u = d_max > d_min ? clamp((d - d_min) / (d_max - d_min), 0.0, 1.0) : 0.0;
    return float2(u, clamp(rho / h, 0.0, 1.0));
}

// Transmittance from `(r, mu)` to the top boundary -- from the table,
// bilinear.
float3 sample_transmittance(float r, float mu)
{
    float2 uv = r_mu_to_uv(r, mu);
    float2 coord = float2(unit_to_texture(uv.x, TRANSMITTANCE_WIDTH),
                          unit_to_texture(uv.y, TRANSMITTANCE_HEIGHT));
    return transmittance_lut.SampleLevel(lut_sampler, coord, 0.0).rgb;
}

// Scattering (without absorption) at height `h` -- the twin of
// `atmosphere::scattering`.
//
// Not the same as `extinction`: ozone does not scatter at all, and Mie
// scatters less than it extinguishes. The source term uses scattering -- a
// photon that was eaten does not fly on into the sky.
float3 scattering(float h)
{
    float3 d = density(h);
    float mie = air.mie.x * d.y;
    return air.rayleigh.rgb * d.x + float3(mie, mie, mie);
}

// Direction number `k` from a uniform 8x8 grid on the sphere -- the twin of
// `atmosphere::sphere_direction`.
float3 sphere_direction(uint k)
{
    float i = 0.5 + float(k / 8u);
    float j = 0.5 + float(k % 8u);
    float theta = 2.0 * 3.14159265358979 * i / 8.0;
    float phi = acos(1.0 - 2.0 * j / 8.0);
    return float3(sin(phi) * cos(theta), sin(phi) * sin(theta), cos(phi));
}

// The multiple-scattering table: 32x32, the `u` axis being the cosine of the
// Sun's zenith angle and the `v` axis the height above the surface. Linear in
// both, and that is not laziness: the non-linearity in the transmittance table
// existed for the tangent ray, i.e. for the sharp edge of the horizon. There
// is no sharp edge here -- everything is already averaged over the sphere of
// directions.
//
// Computed once per set of air parameters, like transmittance: neither the
// camera nor the direction to the Sun appears in the axes (rule 5 of stage S).
//
// RGB is `psi`, the contribution of the second and later orders. **Alpha is
// the largest of the three channels of `f`**, the fraction one scattering
// event returns to the same point. It lives there for checking and is not
// hidden: `psi = L2/(1 - f)` converges only for `f < 1`, and the claim "the
// energy does not grow" is checked with exactly that number. The alpha channel
// is in this texture anyway, so it costs nothing.
[shader("compute")]
[numthreads(8, 8, 1)]
void multiscatter_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= MULTISCATTER_SIZE || id.y >= MULTISCATTER_SIZE) { return; }

    float bottom = air.shape.z;
    float top = air.shape.w;

    float u = float(id.x) / float(MULTISCATTER_SIZE - 1u);
    float v = float(id.y) / float(MULTISCATTER_SIZE - 1u);
    float mu_s = clamp(u * 2.0 - 1.0, -1.0, 1.0);
    float altitude = v * (top - bottom);
    float r = bottom + altitude;

    // The Sun in the xz plane, the point on the z axis.
    float3 sun = float3(sqrt(max(0.0, 1.0 - mu_s * mu_s)), 0.0, mu_s);

    // The starting point's `rho2` **through the height** rather than through a
    // difference of squared radii: at surface level the latter is zero, and in
    // `f32` its sign is decided by rounding. A downward ray then does not stop
    // at the ground but goes through the planet -- and row 0 of the table
    // diverged from the twin twofold while the rest agreed to 0.05%. Likewise
    // `shell2` as a product, not a difference.
    float rho2 = altitude * (2.0 * bottom + altitude);
    float shell2 = (top - bottom) * (top + bottom);

    float3 second = float3(0.0, 0.0, 0.0);
    float3 fraction = float3(0.0, 0.0, 0.0);

    for (uint k = 0u; k < MULTISCATTER_DIRECTIONS; ++k)
    {
        float3 w = sphere_direction(k);
        float mu = w.z;

        float span = distance_to_top(r, mu, rho2, shell2);
        float ground = distance_to_ground(r, mu, rho2);
        if (ground >= 0.0) { span = min(span, ground); }
        float step = span / float(MULTISCATTER_STEPS);

        float3 throughput = float3(1.0, 1.0, 1.0);
        for (uint s = 0u; s < MULTISCATTER_STEPS; ++s)
        {
            float t = (float(s) + 0.5) * step;
            float3 point = float3(t * w.x, t * w.y, r + t * w.z);
            // The sample's `rho2`, also without a difference of squares:
            // `|p + t*w|^2 - bottom^2 = rho2 + 2*t*r*mu + t^2`.
            float rho2_here = max(0.0, rho2 + 2.0 * t * r * mu + t * t);
            float radius = sqrt(max(0.0, rho2_here + bottom * bottom));
            // The height from the same `rho2`: a sum of large numbers does not cancel.
            float h = rho2_here / (radius + bottom);
            float mu_s_here = dot(point, sun) / max(radius, 1.0);

            // The planet's shadow: below the point's horizon there is no direct
            // light at all. That is where the night side comes from -- nobody
            // draws it separately.
            bool lit = distance_to_ground(radius, mu_s_here, rho2_here) < 0.0;
            float3 to_sun = lit ? sample_transmittance(radius, mu_s_here)
                                : float3(0.0, 0.0, 0.0);

            float3 sigma_s = scattering(h);
            float3 sigma_e = extinction(h);

            float3 step_transmittance = exp(-sigma_e * step);
            float3 gain = (float3(1.0) - step_transmittance) / max(sigma_e, TINY);

            // A uniform phase function -- that is the paper's approximation.
            float uniform_phase = 1.0 / (4.0 * PI);
            second += throughput * sigma_s * to_sun * uniform_phase * gain;
            // The fraction: the medium glows with unit radiance from every
            // side, so the source is simply `sigma_s`.
            fraction += throughput * sigma_s * gain;

            throughput *= step_transmittance;
        }

        // Reflection from the surface (T7h). A ray that hit the ground returns
        // a Lambertian `albedo/pi * E`, and returns it **after** the whole
        // path, i.e. multiplied by whatever is left of the ray.
        //
        // For the fraction the source is the same, but the illumination is
        // isotropic with unit radiance: `integral cos/pi dw = 1`, so a
        // Lambertian surface gives back exactly `albedo`. Without this term the
        // series would count reflection only in the second order and not in the
        // third and beyond.
        if (ground >= 0.0)
        {
            float3 hit = float3(ground * w.x, ground * w.y, r + ground * w.z);
            float mu_s_ground = dot(normalize(hit), sun);
            if (mu_s_ground > 0.0)
            {
                second += throughput * air.ground.rgb / PI
                        * sample_transmittance(bottom, mu_s_ground) * mu_s_ground;
            }
            fraction += throughput * air.ground.rgb;
        }
    }

    // The average over the sphere: `4pi/N` of solid angle per direction,
    // divided by the `4pi` of the averaging itself. `1/N` is left.
    second /= float(MULTISCATTER_DIRECTIONS);
    fraction /= float(MULTISCATTER_DIRECTIONS);

    float3 psi = second / max(float3(1.0, 1.0, 1.0) - fraction, float3(1.0e-6));
    multiscatter_out.Store(id.xy, float4(psi, max(fraction.r, max(fraction.g, fraction.b))));
}

// ---------------------------------------------------------------------------
// The sky (S4)
// ---------------------------------------------------------------------------

// Multiple scattering at the point `(r, mu_s)` -- from the table, bilinear.
float3 sample_multiscatter(float r, float mu_s)
{
    float bottom = air.shape.z;
    float top = air.shape.w;
    float u = clamp(mu_s * 0.5 + 0.5, 0.0, 1.0);
    float v = clamp((r - bottom) / (top - bottom), 0.0, 1.0);
    float2 coord = float2(unit_to_texture(u, MULTISCATTER_SIZE),
                          unit_to_texture(v, MULTISCATTER_SIZE));
    return multiscatter_lut.SampleLevel(lut_sampler, coord, 0.0).rgb;
}

// The view direction from unit coordinates of the sky-view table -- the twin
// of `atmosphere::skyview_uv`. Returns `(mu_v, cos_azimuth)`.
float2 skyview_uv(float r, float2 uv)
{
    float bottom = air.shape.z;
    float rho2 = max(0.0, r * r - bottom * bottom);
    float beta = acos(clamp(sqrt(rho2) / r, -1.0, 1.0));
    float zenith_horizon = PI - beta;

    float zenith;
    if (uv.y < 0.5)
    {
        float c = 1.0 - 2.0 * uv.y;
        zenith = zenith_horizon * (1.0 - c * c);
    }
    else
    {
        float c = 2.0 * uv.y - 1.0;
        zenith = zenith_horizon + beta * c * c;
    }
    return float2(cos(zenith), 1.0 - 2.0 * uv.x * uv.x);
}

// Sky-view table coordinates from a view direction -- the twin of
// `atmosphere::skyview_coords`, i.e. the inverse of `skyview_uv`.
//
// Both sides compute `rho2` the same way, which is why writing and reading the
// table agree bitwise where it matters. Precision here is secondary: `beta` is
// close to pi/2, where `acos` is nearly linear, and a 30% error in `rho2`
// gives 1e-4 radian.
float2 skyview_coords(float bottom, float r, float mu_v, float cos_azimuth)
{
    float rho2 = max(0.0, r * r - bottom * bottom);
    float beta = acos(clamp(sqrt(rho2) / r, -1.0, 1.0));
    float zenith_horizon = PI - beta;
    float zenith = acos(clamp(mu_v, -1.0, 1.0));

    float v;
    if (zenith <= zenith_horizon)
    {
        float c = zenith_horizon > 0.0
            ? 1.0 - sqrt(max(0.0, 1.0 - zenith / zenith_horizon))
            : 0.0;
        v = c * 0.5;
    }
    else
    {
        float c = beta > 0.0 ? sqrt(clamp((zenith - zenith_horizon) / beta, 0.0, 1.0)) : 0.0;
        v = 0.5 + c * 0.5;
    }
    float u = sqrt(max(0.0, (1.0 - cos_azimuth) * 0.5));
    return float2(clamp(u, 0.0, 1.0), clamp(v, 0.0, 1.0));
}

// The Rayleigh phase function -- the twin of `atmosphere::rayleigh_phase`.
float rayleigh_phase(float cos_theta)
{
    return 3.0 / (16.0 * PI) * (1.0 + cos_theta * cos_theta);
}

// The Mie phase function (Henyey-Greenstein) -- the twin of
// `atmosphere::mie_phase`.
float mie_phase(float cos_theta, float g)
{
    float denominator = 1.0 + g * g - 2.0 * g * cos_theta;
    return (1.0 - g * g) / (4.0 * PI * pow(max(denominator, 1.0e-6), 1.5));
}

// The scattered light along a ray -- **one implementation for every path**.
//
// It takes the position relative to the body centre, the direction and the
// already computed length of the segment in air. Both the sky-view table and
// the fullscreen pass call exactly this: two copies of the arithmetic would
// diverge, and the sky from the surface would stop matching the sky from orbit
// precisely where the camera crosses the boundary.
//
// `rho2` is the starting point's `r^2 - bottom^2`; whoever knows it more
// precisely passes it in (see the warning above).
float3 raymarch(float3 pos, float3 dir, float3 sun, float rho2, float span, uint steps)
{
    float bottom = air.shape.z;
    float r = length(pos);
    float mu = dot(pos, dir) / max(r, 1.0);

    // The scattering angle is constant along the ray: both directions are fixed.
    float cos_theta = dot(dir, sun);
    float phase_r = rayleigh_phase(cos_theta);
    float phase_m = mie_phase(cos_theta, air.mie.w);

    float step = span / float(steps);
    float3 throughput = float3(1.0, 1.0, 1.0);
    float3 light = float3(0.0, 0.0, 0.0);
    for (uint s = 0u; s < steps; ++s)
    {
        float t = (float(s) + 0.5) * step;
        float3 point = pos + t * dir;
        float rho2_here = max(0.0, rho2 + 2.0 * t * r * mu + t * t);
        float radius = sqrt(max(0.0, rho2_here + bottom * bottom));
        float h = rho2_here / (radius + bottom);
        float mu_s_here = dot(point, sun) / max(radius, 1.0);

        bool lit = distance_to_ground(radius, mu_s_here, rho2_here) < 0.0;
        float3 to_sun = lit ? sample_transmittance(radius, mu_s_here)
                            : float3(0.0, 0.0, 0.0);
        float3 psi = sample_multiscatter(radius, mu_s_here);

        float3 d = density(h);
        float3 sigma_e = extinction(h);
        float3 sigma_r = air.rayleigh.rgb * d.x;
        float sigma_m = air.mie.x * d.y;

        // Direct light gets each component's own phase function; the multiple
        // scattering is already averaged over the sphere and so has no phase.
        float3 source = (sigma_r * phase_r + float3(sigma_m * phase_m)) * to_sun
                      + (sigma_r + float3(sigma_m)) * psi;
        float3 step_transmittance = exp(-sigma_e * step);
        light += throughput * source * (float3(1.0) - step_transmittance) / max(sigma_e, TINY);
        throughput *= step_transmittance;
    }
    return light;
}

// The same, but from sky-view table angles -- the twin of
// `atmosphere::Model::sky_view`.
//
// The coordinate system is local: `up = (0, 0, 1)`, the Sun in the `xz` plane.
// The view is given by the pair `(mu_v, cos_azimuth)`, i.e. exactly what the
// table's axes hold -- and nothing is lost: the physics depends only on
// `dot(view, sun)` and the zenith angles of both, and the sign of the azimuth
// does not enter them.
//
// The ray stops at the surface and adds nothing to it: the surface is drawn by
// the frame, not by the sky-view table.
float3 raymarch_sky(float r, float mu_s, float mu_v, float cos_azimuth, uint steps)
{
    float bottom = air.shape.z;
    float top = air.shape.w;

    float3 sun = float3(sqrt(max(0.0, 1.0 - mu_s * mu_s)), 0.0, mu_s);
    float sin_v = sqrt(max(0.0, 1.0 - mu_v * mu_v));
    float sin_azimuth = sqrt(max(0.0, 1.0 - cos_azimuth * cos_azimuth));
    float3 w = float3(sin_v * cos_azimuth, sin_v * sin_azimuth, mu_v);

    float rho2 = max(0.0, r * r - bottom * bottom);
    float shell2 = (top - bottom) * (top + bottom);
    float span = distance_to_top(r, mu_v, rho2, shell2);
    float ground = distance_to_ground(r, mu_v, rho2);
    if (ground >= 0.0) { span = min(span, ground); }

    return raymarch(float3(0.0, 0.0, r), w, sun, rho2, span, steps);
}

// The sky-view table: 192x108, view direction -> scattered light.
//
// Computed **once per frame**, because it depends on the camera position and
// the direction to the Sun (rule 5 of stage S). The cost depends on neither
// the number of vessels nor anything else in the scene: there is one camera.
[shader("compute")]
[numthreads(8, 8, 1)]
void skyview_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= SKYVIEW_WIDTH || id.y >= SKYVIEW_HEIGHT) { return; }

    float r = frame.view.x;
    float mu_s = frame.view.y;

    float2 uv = float2(float(id.x) / float(SKYVIEW_WIDTH - 1u),
                       float(id.y) / float(SKYVIEW_HEIGHT - 1u));
    float2 angles = skyview_uv(r, uv);

    float3 light = raymarch_sky(r, mu_s, angles.x, angles.y, SKYVIEW_STEPS);
    skyview_out.Store(id.xy, float4(light, 1.0));
}

// ---------------------------------------------------------------------------
// The sky in the frame (S4b)
// ---------------------------------------------------------------------------

// How many steps the fullscreen march takes when the camera is outside the
// air.
//
// Fewer than in the table: there one ray serves a whole sector of sky and
// lives the whole frame, here every pixel has its own. Thirty-two against
// sixteen -- a difference visible at the limb and nowhere else, and S6
// measures it.
static const uint SKY_OUTSIDE_STEPS = 16u;

struct SkyVertex
{
    float4 position : SV_Position;
    // Frame coordinates, [-1, 1]. The pixel ray is built from them.
    float2 ndc : NDC;
};

// A fullscreen triangle -- one, not two: the shared edge of two triangles
// gives a seam where the derivatives are computed differently, and that has
// cost a visible stripe before.
//
// There are no vertex buffers, as with the patches (R6a): the pipeline
// supplies the vertex index.
[shader("vertex")]
SkyVertex vertex_sky(uint id : SV_VertexID)
{
    float2 ndc = float2(float((id << 1u) & 2u), float(id & 2u)) * 2.0 - 1.0;
    SkyVertex output;
    // `z = 0` is the far plane in reversed-Z. This pass's depth test is
    // `Always` anyway and there is no write: the sky is drawn first and
    // everything after it lands on top.
    output.position = float4(ndc, 0.0, 1.0);
    output.ndc = ndc;
    return output;
}

// The pixel ray in world axes.
float3 pixel_ray(float2 ndc)
{
    return normalize(frame.forward.xyz
                   + frame.right.xyz * ndc.x * frame.right.w
                   + frame.screen_up.xyz * ndc.y * frame.screen_up.w);
}

// The sky from the table -- when the camera is **inside** the air.
//
// One `SampleLevel` per pixel instead of a march, which is exactly what the
// table exists for: from the surface the sky costs as much as a texture.
[shader("fragment")]
float4 fragment_sky_inside(SkyVertex input) : SV_Target
{
    float bottom = air.shape.z;
    float r = frame.view.x;
    float3 dir = pixel_ray(input.ndc);
    float3 up = frame.eye.xyz / max(r, 1.0);
    float3 sun = frame.sun.xyz;

    float mu_v = dot(dir, up);
    float mu_s = frame.view.y;

    // The azimuth between the view and the Sun, with the vertical components
    // of both already removed. The degenerate cases (looking at the zenith, the
    // Sun at the zenith) give a zero horizontal vector; the azimuth is
    // undefined there and any value does, because the `sin` of the zenith angle
    // is zero too.
    float3 dir_h = dir - mu_v * up;
    float3 sun_h = sun - mu_s * up;
    float dir_len = length(dir_h);
    float sun_len = length(sun_h);
    float cos_azimuth = (dir_len > 1.0e-6 && sun_len > 1.0e-6)
        ? dot(dir_h, sun_h) / (dir_len * sun_len)
        : 1.0;

    float2 uv = skyview_coords(bottom, r, mu_v, cos_azimuth);
    float2 coord = float2(unit_to_texture(uv.x, SKYVIEW_WIDTH),
                          unit_to_texture(uv.y, SKYVIEW_HEIGHT));
    float3 light = skyview_lut.SampleLevel(lut_sampler, coord, 0.0).rgb;
    return float4(light * frame.view.z, 1.0);
}

// The sky by marching -- when the camera is **outside** the air.
//
// The table does not work here, and not because of precision: its zenith axis
// splits directions by the observer's horizon, and outside the atmosphere
// there is no horizon in that sense -- the air occupies a small disc in the
// frame rather than a hemisphere. So the ray first enters the shell and only
// then marches; outside the disc of air it draws nothing.
[shader("fragment")]
float4 fragment_sky_outside(SkyVertex input) : SV_Target
{
    float bottom = air.shape.z;
    float top = air.shape.w;
    float3 pos = frame.eye.xyz;
    float3 dir = pixel_ray(input.ndc);
    float3 sun = frame.sun.xyz;

    float r = length(pos);
    float mu = dot(pos, dir) / max(r, 1.0);
    float rho2 = max(0.0, r * r - bottom * bottom);
    float shell2 = (top - bottom) * (top + bottom);

    // Whether the ray meets the shell of air at all. The discriminant here is
    // against the TOP sphere, which is far from a tangent, so there is no
    // cancellation.
    float discriminant = r * r * mu * mu + (shell2 - rho2);
    if (discriminant < 0.0 || mu > 0.0) { discard; }
    float root = sqrt(discriminant);
    float entry = -r * mu - root;
    if (entry < 0.0) { discard; }

    // Onwards from the entry point: to the surface or straight through.
    float3 start = pos + entry * dir;
    float start_r = length(start);
    float start_mu = dot(start, dir) / max(start_r, 1.0);
    // At the entry the radius equals the top boundary exactly by construction,
    // so `rho2` there is `shell2` and need not be computed by subtraction.
    float span = distance_to_top(start_r, start_mu, shell2, shell2);
    float ground = distance_to_ground(start_r, start_mu, shell2);
    if (ground >= 0.0) { span = min(span, ground); }

    float3 light = raymarch(start, dir, sun, shell2, span, SKY_OUTSIDE_STEPS);
    return float4(light * frame.view.z, 1.0);
}

// ---------------------------------------------------------------------------
// Aerial perspective (S5)
// ---------------------------------------------------------------------------

// The distance of slice `s` from the camera.
//
// **Quadratic, not uniform.** The volume stretches thousands of kilometres --
// the longest ray that stays in air at all -- while what is interesting in it
// happens near the camera. Uniform slices would put the first at seventy
// kilometres, i.e. nothing closer than that would exist.
float aerial_distance(float slice)
{
    float w = slice / float(AERIAL_Z - 1u);
    float near = frame.eye.w;
    return near + (frame.view.w - near) * w * w;
}

// The volume: one ray per column, slices along it.
//
// One thread per `(x, y)` with a loop over slices rather than a thread per
// texel: neighbouring slices lie on the same ray, and computing each afresh
// from the camera would do the same work thirty-two times.
[shader("compute")]
[numthreads(8, 8, 1)]
void aerial_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= AERIAL_XY || id.y >= AERIAL_XY) { return; }

    float bottom = air.shape.z;
    float top = air.shape.w;
    float3 pos = frame.eye.xyz;
    float3 sun = frame.sun.xyz;

    float2 ndc = (float2(float(id.x) + 0.5, float(id.y) + 0.5) / float(AERIAL_XY)) * 2.0 - 1.0;
    float3 dir = pixel_ray(ndc);

    float r = length(pos);
    float mu = dot(pos, dir) / max(r, 1.0);
    float rho2 = max(0.0, r * r - bottom * bottom);
    float shell2 = (top - bottom) * (top + bottom);

    // How far there is air at all: to the surface or to the top boundary.
    float limit = distance_to_top(r, mu, rho2, shell2);
    float ground = distance_to_ground(r, mu, rho2);
    if (ground >= 0.0) { limit = min(limit, ground); }

    float cos_theta = dot(dir, sun);
    float phase_r = rayleigh_phase(cos_theta);
    float phase_m = mie_phase(cos_theta, air.mie.w);

    float3 throughput = float3(1.0, 1.0, 1.0);
    float3 light = float3(0.0, 0.0, 0.0);
    float previous = 0.0;

    for (uint slice = 0u; slice < AERIAL_Z; ++slice)
    {
        float target = min(aerial_distance(float(slice)), limit);
        float step = max(0.0, target - previous) / float(AERIAL_SLICE_STEPS);

        for (uint k = 0u; k < AERIAL_SLICE_STEPS; ++k)
        {
            float t = previous + (float(k) + 0.5) * step;
            float3 point = pos + t * dir;
            float rho2_here = max(0.0, rho2 + 2.0 * t * r * mu + t * t);
            float radius = sqrt(max(0.0, rho2_here + bottom * bottom));
            float h = rho2_here / (radius + bottom);
            float mu_s_here = dot(point, sun) / max(radius, 1.0);

            bool lit = distance_to_ground(radius, mu_s_here, rho2_here) < 0.0;
            float3 to_sun = lit ? sample_transmittance(radius, mu_s_here)
                                : float3(0.0, 0.0, 0.0);
            float3 psi = sample_multiscatter(radius, mu_s_here);

            float3 d = density(h);
            float3 sigma_e = extinction(h);
            float3 sigma_r = air.rayleigh.rgb * d.x;
            float sigma_m = air.mie.x * d.y;

            float3 source = (sigma_r * phase_r + float3(sigma_m * phase_m)) * to_sun
                          + (sigma_r + float3(sigma_m)) * psi;
            float3 step_transmittance = exp(-sigma_e * step);
            light += throughput * source * (float3(1.0) - step_transmittance)
                   / max(sigma_e, TINY);
            throughput *= step_transmittance;
        }
        previous = target;

        uint3 texel = uint3(id.x, id.y, slice);
        aerial_inscatter_out.Store(texel, float4(light * frame.view.z, 1.0));
        aerial_transmittance_out.Store(texel, float4(throughput, 1.0));
    }
}

// The distance to whatever is drawn in this pixel, metres along the ray.
//
// Returns -1 if nothing is drawn in this depth range: the buffer is cleared to
// zero, and zero in reversed-Z means infinity.
float geometry_distance(int2 pixel, float3 dir)
{
    float z_ndc = depth_texture.Load(int3(pixel, 0)).x;
    if (z_ndc <= 0.0) { return -1.0; }
    // `z` here is along the view axis; the pixel ray is tilted, so it is
    // divided by the cosine between them. There is nothing to divide by zero:
    // the field of view is under 180 degrees.
    float along_axis = range.depth.y / (z_ndc + range.depth.x);
    return along_axis / max(dot(dir, frame.forward.xyz), 1.0e-3);
}

// The coordinate in the volume from a pixel and a distance.
float3 aerial_coord(float2 ndc, float distance)
{
    float near = frame.eye.w;
    float w = sqrt(clamp((distance - near) / max(frame.view.w - near, 1.0), 0.0, 1.0));
    return float3(ndc.x * 0.5 + 0.5,
                  ndc.y * 0.5 + 0.5,
                  unit_to_texture(w, AERIAL_Z));
}

// The first of the two composition calls: multiply the frame by the
// transmittance.
//
// Two calls rather than one, and not out of laziness: `background*T + L` in a
// single pass needs dual-source blending, i.e. another device feature and a
// `@blend_src` in WGSL that the compiler does not emit. Two fullscreen
// triangles cost less than depending on both.
[shader("fragment")]
float4 fragment_aerial_multiply(SkyVertex input) : SV_Target
{
    float3 dir = pixel_ray(input.ndc);
    float distance = geometry_distance(int2(input.position.xy), dir);
    if (distance < 0.0) { discard; }
    float3 t = aerial_transmittance_lut.SampleLevel(lut_sampler,
                                                    aerial_coord(input.ndc, distance), 0.0).rgb;
    return float4(t, 1.0);
}

// The second: add what was scattered along the way.
[shader("fragment")]
float4 fragment_aerial_add(SkyVertex input) : SV_Target
{
    float3 dir = pixel_ray(input.ndc);
    float distance = geometry_distance(int2(input.position.xy), dir);
    if (distance < 0.0) { discard; }
    float3 l = aerial_inscatter_lut.SampleLevel(lut_sampler,
                                                aerial_coord(input.ndc, distance), 0.0).rgb;
    return float4(l, 1.0);
}
