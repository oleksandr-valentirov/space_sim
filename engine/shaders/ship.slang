// The ship in the frame (stage V, step V2).
//
// Positions in the vertex buffer are already camera-relative -- the same
// convention as in line.slang and sphere.slang: the CPU subtracted the camera
// position in double and narrowed the result (ROADMAP F4, engine::camera).
// Only the projection is left here.
//
// WARNING: **everything here is in CAMERA axes** -- positions, normals and the
//   direction to the star. Positions arrive that way by construction
//   (`Camera::relative` rotates rather than merely subtracting), and since T5c
//   the shader takes the direction to the eye from them: `view = -position`.
//   So the normal and the star must be in the same space, and the same CPU
//   pass rotates them. Doing it in the shader would mean a second matrix per
//   ship, i.e. another uniform with a dynamic offset -- and there are fifteen
//   hundred vertices here, not a million.
//
// The lighting is Cook-Torrance with GGX (T5c), a verbatim twin of
// `engine::brdf`. Both must be edited; the oracle is that the numbers agree,
// and `tests/hull.rs` provides it.
//
// What is still missing from the model of PROJECT.md §7: planetshine by an SH
// probe (T6) and the direction to the star from the ephemeris (debt D16).
// Ambient stays zero -- there will be no invented "a little light from
// everywhere" here.

// The colour is not in here: it is a vertex attribute, for the same reason as
// in line.slang. There are several ships in a frame, one draw call each, and
// all queue writes happen BEFORE the pass -- so in a uniform the last one
// would win.
struct Uniforms
{
    float4x4 projection;
    float4 light_dir;   // direction TO the star, CAMERA axes, xyz
};

// A dynamic offset selects the depth pass, as with the polylines. One buffer,
// a stride of 256 bytes (frame::PASS_STRIDE).
ConstantBuffer<Uniforms> uniforms;

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float4 colour : COLOR;
    // x -- roughness, y -- metallic. A vertex attribute like the colour and
    // for the same reason: there are several ships in a frame, and in a
    // uniform the last one would win.
    float2 material : MATERIAL;
    // Planetshine (T6): the direction **to the planet** in camera axes and
    // the per-channel irradiance. Per vertex, because two ships can hang over
    // different places; `engine::planetshine` computes it once per ship.
    float3 shine_dir : SHINEDIR;
    float3 shine_rgb : SHINERGB;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
    float4 colour : COLOR;
    nointerpolation float2 material : MATERIAL;
    nointerpolation float3 shine_dir : SHINEDIR;
    nointerpolation float3 shine_rgb : SHINERGB;
    // The vertex position **relative to the camera** -- the fragment takes
    // the direction to the eye from it. Camera-relative is not an optimisation
    // here but the buffer's convention: the position already arrived that way,
    // so the eye is at zero and the direction to it is just `-position`.
    float3 view : VIEWDIR;
};

[shader("vertex")]
VertexOutput vertex_main(VertexInput input)
{
    VertexOutput output;
    output.position = mul(uniforms.projection, float4(input.position, 1.0));
    output.normal = input.normal;
    output.colour = input.colour;
    output.material = input.material;
    output.shine_dir = input.shine_dir;
    output.shine_rgb = input.shine_rgb;
    output.view = -input.position;
    return output;
}

// Normal-incidence reflectance of a dielectric -- `engine::brdf::DIELECTRIC_F0`.
static const float DIELECTRIC_F0 = 0.04;
// The smallest roughness -- `engine::brdf::MIN_ROUGHNESS`. Below it the peak
// of `D` overflows `f32`: infinity in one pixel and zero in the next.
static const float MIN_ROUGHNESS = 0.045;
static const float PI = 3.14159265358979323846;

// GGX / Trowbridge-Reitz -- the twin of `brdf::distribution`.
float distribution(float n_dot_h, float roughness)
{
    float a = max(roughness, MIN_ROUGHNESS);
    a = a * a;
    float a2 = a * a;
    float d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// Height-correlated Smith, already divided by `4(n.l)(n.v)` -- the twin of
// `brdf::visibility`. A separate denominator would give zero over zero at
// grazing angles; the same class as `max(x, 1e-30)` in the air.
float visibility(float n_dot_v, float n_dot_l, float roughness)
{
    float a = max(roughness, MIN_ROUGHNESS);
    a = a * a;
    float a2 = a * a;
    float v = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    float l = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(v + l, 1e-30);
}

// Schlick -- the twin of `brdf::fresnel`.
float3 fresnel(float3 f0, float v_dot_h)
{
    float t = saturate(1.0 - v_dot_h);
    float t5 = t * t * t * t * t;
    return f0 + (float3(1.0) - f0) * t5;
}

// How much light reaches the eye per unit of irradiance -- the twin of
// `brdf::radiance`.
float3 radiance(float3 n, float3 v, float3 l, float3 base, float roughness, float metallic)
{
    float n_dot_l = dot(n, l);
    float n_dot_v = dot(n, v);
    if (n_dot_l <= 0.0 || n_dot_v <= 0.0)
    {
        return float3(0.0);
    }

    float3 h = normalize(v + l);
    float n_dot_h = saturate(dot(n, h));
    float v_dot_h = saturate(dot(v, h));

    // A metal has no diffuse reflection, and its `F0` is the base colour.
    float3 f0 = lerp(float3(DIELECTRIC_F0), base, metallic);
    float3 f = fresnel(f0, v_dot_h);
    float3 specular = distribution(n_dot_h, roughness)
        * visibility(n_dot_v, n_dot_l, roughness) * f;
    float3 diffuse = (float3(1.0) - f) * (1.0 - metallic) * base / PI;

    return (diffuse + specular) * n_dot_l;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    float3 n = normalize(input.normal);
    float3 v = normalize(input.view);

    // WARNING: the normal is flipped towards the eye, and that is not
    // cosmetics: the ship pipeline draws **with no face culling**, so the
    // inner side of the shell legitimately reaches the frame. Without the flip
    // the BRDF answers such a fragment with zero, and the hull gets black
    // patches where a lit surface should be. `patch.slang` does the same with
    // the facet normal.
    if (dot(n, v) < 0.0)
    {
        n = -n;
    }

    float roughness = input.material.x;
    float metallic = input.material.y;
    float3 base = input.colour.rgb;

    // The star: unit irradiance.
    float3 out = radiance(n, v, uniforms.light_dir.xyz, base, roughness, metallic);

    // The planet below (T6). There is no ambient and will not be: the shadow
    // side of the hull is lit by exactly what the surface under it reflected
    // and by nothing else (PROJECT.md §7). On the night side `shine_rgb` is
    // zero, so the term vanishes by itself with no branch.
    out += radiance(n, v, normalize(input.shine_dir), base, roughness, metallic)
        * input.shine_rgb;

    return float4(out, 1.0);
}
