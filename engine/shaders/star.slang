// The star background (ROADMAP, stage Z, Z3).
//
// Its own file rather than an addition to `sky.slang`, and the reason is
// traffic rather than taste: that file is large, and D17 is still open in it.
// What lives in `sky.slang` is only the coupling -- the transmittance the
// stars are multiplied by (Z4).
//
// ## Vertex-less, like everything else here
//
// Six vertices per star out of `SV_VertexID`, no vertex buffer: rule 5 of
// stage R. The star's data is read from a storage buffer by its index, which
// is also why a star is two triangles rather than a point primitive -- point
// size is a device-dependent mess, and a quad is the same on all three
// targets.

// One star, as the CPU packs it: a unit direction and a flux relative to
// magnitude zero.
//
// The flux is precomputed rather than the magnitude stored, and the split is
// deliberate: the *asset* keeps magnitudes, because a brightness in the file
// would bake one exposure into it for ever, while the *buffer* keeps the ratio
// 10^(-0.4 m), because that ratio is exposure-independent and the shader then
// needs no exponent at all.
struct Star
{
    // xyz -- unit direction in world axes, w -- flux relative to magnitude 0.
    float4 dir_flux;
};

[[vk::binding(0, 0)]]
StructuredBuffer<Star> stars;

// What changes every frame: where the camera looks and how big a star is.
struct StarView
{
    // The screen axes in world coordinates, exactly as `sky.slang` packs
    // them: `right.w = tan(fovy/2)*aspect`, `up.w = tan(fovy/2)`. The same
    // packing on purpose -- this shader inverts what `pixel_ray` does there,
    // and two different conventions for one basis is how signs get lost.
    float4 right;
    float4 up;
    float4 forward;
    // x -- the star's diameter in pixels, y and z -- the viewport in pixels,
    // w -- the radiance of a magnitude-zero star.
    float4 params;
};

[[vk::binding(1, 0)]]
ConstantBuffer<StarView> view;

struct StarVertex
{
    float4 position : SV_Position;
    // Where in the quad this pixel is, in [-1, 1] on both axes -- the falloff
    // is computed from it rather than from the pixel coordinate, so a star
    // looks the same wherever it lands.
    float2 offset : OFFSET;
    // `nointerpolation`: one number for the whole quad, and interpolating it
    // would make one corner of every star brighter than the other.
    nointerpolation float flux : FLUX;
};

[shader("vertex")]
StarVertex vertex_star(uint id: SV_VertexID)
{
    StarVertex output;

    Star star = stars[id / 6u];
    float3 dir = star.dir_flux.xyz;

    // The two triangles of the quad, as corners in [-1, 1].
    uint corner = id % 6u;
    // 0,1,2 and 2,1,3 -- the second triangle shares the diagonal.
    uint index = corner < 3u ? corner : (corner == 3u ? 2u : (corner == 4u ? 1u : 3u));
    float2 offset = float2(float(index & 1u) * 2.0 - 1.0, float(index >> 1u) * 2.0 - 1.0);

    // The projection, inverting `pixel_ray`: how far along the view axis, and
    // how far across it.
    float ahead = dot(dir, view.forward.xyz);
    float2 ndc = float2(dot(dir, view.right.xyz) / max(view.right.w, 1.0e-6),
                        dot(dir, view.up.xyz) / max(view.up.w, 1.0e-6));

    // Behind the camera, or exactly at ninety degrees: pushed off screen
    // rather than branched away, because a vertex shader cannot discard.
    //
    // Far outside the clip volume with `w = 1`, NOT `float4(0, 0, 0, 0)`. The
    // latter looks like a tidy "nothing" and is a division by zero in the
    // perspective divide -- undefined, and undefined here means one driver
    // clips it and the next draws a star in the middle of the screen.
    if (ahead <= 1.0e-6)
    {
        output.position = float4(1.0e4, 1.0e4, 0.0, 1.0);
        output.offset = float2(0.0, 0.0);
        output.flux = 0.0;
        return output;
    }
    ndc /= ahead;

    // The quad's half-size in NDC. Pixels to NDC is a factor of two over the
    // viewport, because NDC spans [-1, 1] across it.
    float2 half_size = float2(view.params.x / view.params.y, view.params.x / view.params.z);

    // **z = 0, and the depth test is `Always`.** Reversed-Z puts infinity at
    // zero, which is where the stars are; the pass writes no depth, so nothing
    // has to be argued with about it. Everything drawn afterwards -- planets,
    // ships, the air -- lands on top by the ordinary test.
    output.position = float4(ndc + offset * half_size, 0.0, 1.0);
    output.offset = offset;
    output.flux = star.dir_flux.w;
    return output;
}

[shader("fragment")]
float4 fragment_star(StarVertex input) : SV_Target
{
    // A round star, not a square one, and the falloff smooth rather than a
    // hard edge: a star is a point source blurred by the optics and by the
    // pixel grid, and a hard-edged disc reads as a bug even at two pixels
    // across.
    float r2 = dot(input.offset, input.offset);
    float fade = max(0.0, 1.0 - r2);
    fade = fade * fade;

    // Linear radiance into an HDR target: the tonemapper and the exposure
    // (Z1) are the only things that turn this into a colour, and they run
    // once for the whole frame.
    float radiance = view.params.w * input.flux * fade;
    return float4(radiance, radiance, radiance, 1.0);
}
