// Tonemapper: HDR frame -> the target's range (ROADMAP, T5c3).
//
// The pass exists because materials brought values above one into the frame.
// The hull's specular highlight at `roughness = 0.35` reaches about **3.7** --
// without this pass it would clip at 255 and become a flat white disc with
// neither shape nor edge.
//
// ## Why a separate pass rather than a line in every shader
//
// Aerial perspective composition does `dst*T` and `dst + L` with the frame
// (S5), i.e. it **reads** what is already drawn. Tonemapping in each shader
// would multiply and add already compressed numbers, and the air would stop
// being physics. So the scene goes into `Rgba16Float` and the compression
// happens once, at the end.
//
// ## The curve: identity below the knee, compression above
//
// `y = x` for `x <= K`; then `y = 1 - (1-K)^2/(x - 2K + 1)`. Continuous
// together with its first derivative (at `K` both halves give `K` and slope
// 1), tending to one at infinity.
//
// WARNING: identity below the knee is the main property, not a
//   simplification. All the diffuse work of stage T lives there: the Moon's
//   reflectance 0.02 to 0.25, the mosaic colour, the material rule. A curve
//   that touches those values (Reinhard, ACES, PBR Neutral) would change every
//   one of them -- forcing every oracle to be remeasured and robbing the "byte
//   against a measured number" comparison of its meaning (T5b). Here they come
//   out of the pass **bit for bit the same**.
//
// This curve has exactly one duty: to keep the highlight from collapsing into
// a white blob. A film characteristic with lifted shadows, should it ever be
// needed, will have to arrive together with exposure and a reference look --
// that is T5d, and it is blocked.
//
// ## Per channel
//
// Each channel is compressed separately, so a very bright coloured highlight
// crawls towards white. A real sensor behaves that way when overexposed; the
// "compress the luminance, keep the hue" variant gives coloured blobs where
// the eye expects white.

static const float KNEE = 0.8;

// The scene frame in linear light. There is no sampler on purpose: the pass
// is one to one in pixels, so `Load` at integer coordinates is exact, and
// filtering here could only spoil it.
Texture2D<float4> scene;

float compress(float x)
{
    if (x <= KNEE)
    {
        return x;
    }
    float d = 1.0 - KNEE;
    return 1.0 - d * d / (x - 2.0 * KNEE + 1.0);
}

struct VertexOutput
{
    float4 position : SV_Position;
};

// A fullscreen **triangle**, not two: the shared edge of two triangles is
// rasterised twice, and at fullscreen that shows.
[shader("vertex")]
VertexOutput vertex_main(uint vertex : SV_VertexID)
{
    float2 xy = float2(float((vertex << 1) & 2u), float(vertex & 2u));
    VertexOutput output;
    output.position = float4(xy * 2.0 - 1.0, 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragment_main(float4 position : SV_Position) : SV_Target
{
    // `SV_Position` in the fragment is the target pixel coordinate, so the
    // orientation matches the texture by construction and nothing needs
    // flipping.
    float3 c = scene.Load(int3(int(position.x), int(position.y), 0)).rgb;
    return float4(compress(c.r), compress(c.g), compress(c.b), 1.0);
}
