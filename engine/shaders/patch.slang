// A cubesphere patch (ROADMAP-PLANETS.md, R1d, R5c).
//
// There is one difference from sphere.slang and it is all here: vertices
// arrive as **offsets from the patch origin** rather than camera-relative
// positions. The CPU subtracts the camera exactly once per patch (in double)
// rather than once per vertex -- that is what the patch is a unit for (rule 1
// of stage R).
//
// So the shader does what the CPU used to: add the offset and rotate the sum
// into camera space. The rotation arrives already multiplied into
// `projection` -- there is no translation in the matrix, because the CPU's
// subtraction has already performed it.
//
// Since R1e the patch geometry is a **unit sphere**, shared by all bodies,
// while a particular body's size and rotation come in a second matrix.
// Otherwise two bodies of different radii would mean two copies of the same
// vertices.
//
// ## Terrain: two entry points, not a branch (R5c)
//
// A body with tiles and a body without are drawn by different pipelines. Not
// an optimisation but a way around an already-caught trap: a runtime switch
// on a uniform in the VERTEX stage was silently read wrong on AMD RADV (ACO)
// (ROADMAP F6), and the frame was empty with no diagnostics at all. The
// second reason is simpler: a smooth body has no tile, and a `textureLoad` at
// an undefined index should not even appear in its program.
//
// The normal differs too, and that is not cosmetics. For a smooth sphere the
// normal is analytic -- the direction from the centre. For terrain no such
// normal exists: the surface is a set of facets the size of a DEM cell, and
// the only honest normal is the geometric one, from screen-space derivatives
// of the world position. It is what shows the terrain as shadows on the
// terminator, which is the point of the step.

struct Uniforms
{
    float4x4 projection;  // projection times the view rotation
    float4x4 model;       // body rotation x its radius, in world axes
    float4 light_dir;     // direction TO the light source, world space
    float4 colour;
    // x -- unit-sphere units per one storage unit of height, i.e.
    // `scale_m / radius_m`. y -- the "colour storage unit -> grey on screen"
    // factor; **zero in it means there is no colour**, and the fragment then
    // falls back to `colour`. z -- how many channels the colour tile has: one
    // (the Moon's brightness) or four (Earth's colour, T7g). w -- spare.
    float4 terrain;
    // Procedural detail (R7c): x -- body radius in metres, y -- the slope
    // storage unit (`tiles::SLOPE_UNIT`), z -- the wavelength of the coarsest
    // octave (`detail::base_m`, i.e. the body radius over a constant, NOT the
    // pyramid depth: that is cooker state), w -- pixels per radian
    // (`lod::focal_px`).
    float4 detail;
};

ConstantBuffer<Uniforms> uniforms;

// Each patch's origin, already relative to the camera and already rotated
// with the body, in WORLD axes, plus its tile index.
//
// A storage buffer rather than an array of uniform buffers: D3D12 does not
// offer the latter at all (PROJECT.md §7, scouting P0), and per-object data
// lives exactly like this there.
struct PatchData
{
    float3 origin;
    uint tile;
    // This patch's window into its tile (R7a): the offset in nodes and the
    // step -- how many tile nodes fall on one patch node.
    //
    // A patch with a tile of its own gives `(0, 0)` and `1`, i.e. an exact
    // `Load` as before R7a. A patch deeper than the pyramid looks into a
    // sub-rectangle of an ancestor, and the step becomes fractional -- hence
    // the bilinear sampling below.
    //
    // The numbers come from `Terrain::window` on the CPU, the same code as
    // `Terrain::height_m`. There is no second computation here on purpose: two
    // independent windows would diverge, and the difference would show as a
    // crack.
    float2 window_origin;
    float window_step;
    // Padding, nothing more. Before stage W the central-difference step lived
    // here; the slope now sits in the tile itself. The word stayed because
    // std430 requires eight-byte alignment before `colour_origin` anyway: the
    // struct without it has the same 48 bytes, only with a hole.
    float pad;
    // The same, but in the **colour** pyramid (T3b): tile index, offset in
    // nodes and step.
    //
    // WARNING: a second window rather than one shared with height, and that is
    //   not duplication: the pyramids have different depths (5 height levels
    //   against 6 colour), so the same patch quite legitimately reads height in
    //   an ancestor and colour in a tile of its own. A shared window would mean
    //   the depths had to agree.
    // WARNING: the order is exactly this -- `float2` **after** the two scalars
    //   -- and that is not taste. In std430 `vec2<f32>` requires eight-byte
    //   alignment, so "uint, float2, float" would leave a hole at the fourth
    //   word and the struct would grow from 48 bytes to 64. This is checked not
    //   by reasoning but against the generated WGSL: the `@align` of every field
    //   must match how `frame.rs` packs it.
    uint colour_tile;
    float colour_step;
    float2 colour_origin;
};

StructuredBuffer<PatchData> patches;

// Terrain tiles are a **bindless texture array**, from the engine's very
// first texture (rule 6 of stage R). There is no sampler beside them, and
// that is not an omission: a tile lies node for node with the patch grid, so
// sampling here is an exact `Load` at an integer index, not filtering.
//
// Two channels, both in the same texel: `x` -- height, `y` -- slope, baked by
// the cooker (stage W, Q3). Splitting them into two textures is unthinkable:
// D19 pays for the NUMBER of textures in the array, so a second array would
// cost as much as all the terrain.
//
// WARNING: a separate bind group (set 1), and that is a wgpu requirement
//   rather than taste: "bind groups may not contain both a binding array and a
//   dynamically offset buffer". And the dynamic offset in group 0 selects the
//   depth pass (R4a), i.e. it is not going anywhere. So the array lives apart.
[[vk::binding(0, 1)]]
Texture2D<int2> tiles[];

// The colour of the same surface -- a second array in the same group (T3b).
//
// No sampler here either, although colour does need filtering: the sampling
// is done by hand with four `Load`s, exactly as in `sample_tile`. The reason
// is not economy -- **sampler filtering does not know about the window**. A
// patch deeper than the colour pyramid reads a sub-rectangle of an ancestor's
// tile, and `colour_origin`/`colour_step` compute its texel coordinates; UVs
// for a sampler would have to be derived from the same numbers, i.e. the same
// arithmetic written twice.
[[vk::binding(1, 1)]]
Texture2D<float4> colours[];

// The patch geometry: vertex offset and normal, both on the UNIT sphere.
//
// A storage buffer rather than vertex attributes (R6a). There is one reason
// and it is all here: stitching levels substitutes a node INDEX, while vertex
// attributes arrive already fetched, so the substitution had to be done with
// an index buffer -- sixteen sets and one draw call per patch. Reading the
// node itself, the shader does the same substitution by arithmetic, and the
// patches merge into one instanced call.
struct PatchVertex
{
    float3 offset;
    float3 normal;
};

StructuredBuffer<PatchVertex> vertices;

// What this instance draws: the geometry slot in the cache and the stitching
// mask.
struct PatchDraw
{
    uint slot;
    uint mask;
};

StructuredBuffer<PatchDraw> draws;

// The patch grid. Must match `cubesphere::SIDE`; the `engine::tests::gpu_driven`
// test compares them, because the constant here and the constant there are two
// places.
static const uint SIDE = 32;
static const uint NODES = SIDE + 1;

// Procedural detail (R7c). The numbers are verbatim `engine::detail`;
// `engine::tests::detail` compares them, because the constant here and the
// constant there are two places.
static const uint OCTAVES = 6u;
static const float STEEPNESS = 0.5;
static const float FADE_LO_PX = 4.0;
static const float FADE_HI_PX = 16.0;

// The material rule (T4). The numbers are verbatim `engine::material`;
// `engine::tests::material` compares them, because the constant here and the
// constant there are two places.
static const float SLOPE_GAIN = 0.30;
static const float SLOPE_REF = 0.15;
static const float RELIEF_GAIN = 0.45;
static const float MIN_TINT = 0.35;
static const float MAX_TINT = 1.80;

struct VertexInput
{
    uint vertex : SV_VertexID;
    uint instance : SV_InstanceID;
};

// A decoded node: its address in the tile and its index in the geometry
// buffer.
struct Node
{
    uint2 grid;   // (a, b) after the stitching substitution
    uint index;   // offset into the geometry buffer
};

// The node for a vertex number in the patch's triangle list.
//
// The order reproduces `cubesphere::indices` literally: cell `(a, b)` with `a`
// as the outer loop, two triangles `v00 v10 v01` and `v01 v10 v11`. They may
// not diverge -- a test compares the frame with what the index buffer drew,
// bitwise.
Node node_of(uint vertex, uint slot, uint mask)
{
    uint triangle = vertex / 3u;
    uint corner = vertex % 3u;
    uint cell = triangle / 2u;
    uint half = triangle % 2u;

    uint a = cell / SIDE;
    uint b = cell % SIDE;

    // The (da, db) corners of both triangles of the cell.
    uint2 first[3] = { uint2(0, 0), uint2(1, 0), uint2(0, 1) };
    uint2 second[3] = { uint2(0, 1), uint2(1, 0), uint2(1, 1) };
    uint2 step = half == 0u ? first[corner] : second[corner];
    a += step.x;
    b += step.y;

    // Stitching: on an edge with a coarser neighbour the odd node does not
    // exist, and wherever it is mentioned the previous even one stands. The
    // same substitution as in `cubesphere::indices`, which is why it must be
    // verbatim here.
    bool odd_on_b = (a % 2u == 1u)
        && ((b == 0u && (mask & 4u) != 0u) || (b == SIDE && (mask & 8u) != 0u));
    bool odd_on_a = (b % 2u == 1u)
        && ((a == 0u && (mask & 1u) != 0u) || (a == SIDE && (mask & 2u) != 0u));
    if (odd_on_b) { a -= 1u; }
    if (odd_on_a) { b -= 1u; }

    Node out;
    out.grid = uint2(a, b);
    out.index = slot * NODES * NODES + a * NODES + b;
    return out;
}

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
    // The position in world axes relative to the camera -- terrain needs it
    // so the fragment can take a geometric normal from it.
    float3 world : WORLD;
    // The same position **before the terrain displacement**, i.e. on the
    // smooth sphere.
    //
    // Needed for exactly one thing: the facet computed from `world` carries
    // both the terrain slope and the break of the sphere's own tessellation,
    // and the second has no business being in the normal. The facet from
    // `base` is that break by itself, so the difference of the two facets
    // leaves the pure terrain slope (T7f).
    float3 base : BASE;
    // Where this pixel lies in the colour tile: the tile index and the
    // coordinate in its texels (T3b).
    //
    // The tile is `nointerpolation`: it is an integer and the same for the
    // whole patch, and interpolating an integer between vertices would give
    // the index of someone else's tile on most pixels. The coordinate, by
    // contrast, is interpolated -- that is what makes the colour continuous
    // inside a cell.
    nointerpolation uint colour_tile : COLOURTILE;
    float2 colour_node : COLOURNODE;
    // The albedo factor from slope and procedural relief (T4).
    //
    // Computed in the VERTEX, and that is a decision rather than convenience:
    // both of its arguments are bitwise identical on a node shared by two
    // patches of different levels, so the colour across a level boundary is
    // continuous by construction. Computed in the fragment from `ddx/ddy` it
    // would be a function of the triangle, i.e. of the patch level.
    float tint : TINT;
};

// The shared part: the vertex offset inside the patch, already rotated and
// scaled.
VertexOutput place(uint slot, PatchVertex vertex, uint2 grid, float3 offset)
{
    PatchData patch = patches[slot];
    float3 origin = patch.origin;
    float3 world = origin + mul(uniforms.model, float4(offset, 0.0)).xyz;

    VertexOutput output;
    // One means "the rule does nothing". A smooth body leaves it as is and
    // terrain overwrites it; on a plain terrain also gives exactly one.
    output.tint = 1.0;
    output.colour_tile = patch.colour_tile;
    // Patch node -> colour tile texel, by the same kind of window as height
    // but **its own**. There is no halo offset here since stage W: both tiles
    // are exactly the patch grid.
    output.colour_node = patch.colour_origin + float2(grid) * patch.colour_step;
    output.position = mul(uniforms.projection, float4(world, 1.0));
    // The same rotation as for the offset: a normal rotated differently from
    // the geometry lights another body. The radius in the matrix only
    // lengthens the normal -- `normalize` in the fragment removes it.
    output.normal = mul(uniforms.model, float4(vertex.normal, 0.0)).xyz;
    output.world = world;
    // The undisplaced vertex of the same patch. The smooth pass puts the same
    // thing here as in `world` -- which is why its facet difference is
    // identically zero.
    output.base = origin + mul(uniforms.model, float4(vertex.offset, 0.0)).xyz;
    return output;
}

[shader("vertex")]
VertexOutput vertex_smooth(VertexInput input)
{
    PatchDraw draw = draws[input.instance];
    Node node = node_of(input.vertex, draw.slot, draw.mask);
    PatchVertex vertex = vertices[node.index];
    return place(draw.slot, vertex, node.grid, vertex.offset);
}

// Height and slope at a patch node, in storage units (R7a; stage W).
//
// A verbatim twin of `Terrain::sample`, exactly as `cull.slang` is a twin of
// `engine::cull`. The oracle is the same: the numbers on both sides must
// agree, and a test compares them.
//
// **One sample for two numbers.** Height and slope live in the same texel and
// share one window, so four `Load`s give both. Before stage W the slope was
// computed here by central difference -- four samples of four `Load`s, twenty
// per node against today's four -- and at a cube corner it gave three
// different answers depending on the face (Q3).
//
// The tile lies node for node with the patch grid, and `a` in it is the row,
// i.e. the texture's `y`. When a patch has a tile of its own `window_step ==
// 1`, the offset is zero, and all four taps fall on one node -- the sampling
// degenerates into the exact `Load` that was here before R7a.
float2 sample_tile(PatchData patch, uint2 grid)
{
    float x = patch.window_origin.x + float(grid.x) * patch.window_step;
    float y = patch.window_origin.y + float(grid.y) * patch.window_step;

    float x0 = floor(x);
    float y0 = floor(y);
    float tx = x - x0;
    float ty = y - y0;

    // Clamping at the top node: the right and bottom edges of a tile have no
    // neighbour inside it. The step divides `SIDE` exactly, so a clamped tap
    // happens precisely at the boundary where `tx` or `ty` is already zero and
    // does not affect the result.
    int xi = int(x0);
    int yi = int(y0);
    int xi1 = min(xi + 1, int(SIDE));
    int yi1 = min(yi + 1, int(SIDE));

    // `Load` takes texture (x, y, mip) = (b, a, 0): `b` is the column, `a` the
    // row. There is no halo offset here: a terrain tile is exactly the patch
    // grid.
    float2 h00 = float2(tiles[patch.tile].Load(int3(yi, xi, 0)).xy);
    float2 h01 = float2(tiles[patch.tile].Load(int3(yi1, xi, 0)).xy);
    float2 h10 = float2(tiles[patch.tile].Load(int3(yi, xi1, 0)).xy);
    float2 h11 = float2(tiles[patch.tile].Load(int3(yi1, xi1, 0)).xy);

    float2 top = h00 * (1.0 - ty) + h01 * ty;
    float2 bottom = h10 * (1.0 - ty) + h11 * ty;
    return top * (1.0 - tx) + bottom * tx;
}

// A hash of three integers into [0, 1) -- the twin of `engine::detail::hash`.
float detail_hash(int x, int y, int z)
{
    uint h = uint(x) * 0x9E3779B1u ^ uint(y) * 0x85EBCA6Bu ^ uint(z) * 0xC2B2AE35u;
    h ^= h >> 15u;
    h = h * 0x2545F491u;
    h ^= h >> 13u;
    h = h * 0x27D4EB2Fu;
    h ^= h >> 16u;
    return float(h >> 8u) / 16777216.0;
}

float detail_smooth(float t)
{
    return t * t * (3.0 - 2.0 * t);
}

// Value noise -- the twin of `engine::detail::value_noise`.
float value_noise(float3 p)
{
    float3 cell = floor(p);
    float3 t = float3(detail_smooth(p.x - cell.x),
                      detail_smooth(p.y - cell.y),
                      detail_smooth(p.z - cell.z));
    int x = int(cell.x);
    int y = int(cell.y);
    int z = int(cell.z);

    float out = 0.0;
    for (int dx = 0; dx < 2; ++dx)
    {
        float wx = dx == 0 ? 1.0 - t.x : t.x;
        for (int dy = 0; dy < 2; ++dy)
        {
            float wy = dy == 0 ? 1.0 - t.y : t.y;
            for (int dz = 0; dz < 2; ++dz)
            {
                float wz = dz == 0 ? 1.0 - t.z : t.z;
                out += detail_hash(x + dx, y + dy, z + dz) * wx * wy * wz;
            }
        }
    }
    return out;
}

// The octave weight -- the twin of `engine::detail::octave_weight`.
//
// It depends on wavelength, distance and focal length, and on NOTHING else.
// In particular not on the patch level: neighbours differ by a level by
// construction, and a criterion that looked at the level would give two
// heights at a shared node.
float octave_weight(float wavelength, float distance, float focal)
{
    float px = wavelength / max(distance, 1.0) * focal;
    if (px <= FADE_LO_PX) { return 0.0; }
    if (px >= FADE_HI_PX) { return 1.0; }
    return detail_smooth((px - FADE_LO_PX) / (FADE_HI_PX - FADE_LO_PX));
}

// Procedural detail at a node -- the twin of `engine::detail::sample`.
//
// Returns two numbers from one pass: `x` -- height in metres for the shape,
// `y` -- dimensionless roughness for the material (T4). The noise, the sample
// points and the fade are shared; only the octave weight differs -- height
// multiplies each by its wavelength, roughness by nothing. A second pass would
// cost as much as the first: everything expensive here is the hashes.
//
// `unit` is the node's unit direction from the body centre, i.e. the normal
// from the geometry buffer. That one, not the position relative to the patch:
// on a shared edge the normal is bitwise single (R2b), so the detail is
// continuous by construction rather than by agreement.
float2 detail_sample(float3 unit, float slope, float distance)
{
    float radius = uniforms.detail.x;
    float base = uniforms.detail.z;
    float focal = uniforms.detail.w;

    float height = 0.0;
    float roughness = 0.0;
    for (uint octave = 0u; octave < OCTAVES; ++octave)
    {
        float wavelength = base / float(1u << octave);
        float weight = octave_weight(wavelength, distance, focal);
        if (weight <= 0.0) { break; }
        float scale = radius / wavelength;
        float signed_noise = value_noise(unit * scale) - 0.5;
        height += signed_noise * STEEPNESS * slope * wavelength * weight;
        roughness += 2.0 * signed_noise * weight;
    }
    return float2(height, roughness);
}

// The albedo factor at a node -- the twin of `engine::material::tint`.
//
// Neither of the first two arguments comes from the asset: the slope is
// metres per metre, the roughness is noise by position on the body. The
// pyramid depth has nowhere to enter here, which is why recooking the asset
// does not repaint the slopes. The third argument is the node's height in
// storage units. Below sea level the rule returns exactly one: it highlights
// slope, and under water the frame shows the sea surface rather than the
// slope of the sea floor. On Earth the floor is measured **steeper** than the
// land, so without this branch the rule would draw mid-ocean ridges over flat
// water.
float material_tint(float slope, float roughness, float height)
{
    if (height < uniforms.terrain.w)
    {
        return 1.0;
    }
    float steep = clamp(slope / SLOPE_REF, 0.0, 1.0);
    return clamp(1.0 + steep * (SLOPE_GAIN + RELIEF_GAIN * roughness), MIN_TINT, MAX_TINT);
}

[shader("vertex")]
VertexOutput vertex_terrain(VertexInput input)
{
    PatchDraw draw = draws[input.instance];
    Node node = node_of(input.vertex, draw.slot, draw.mask);
    PatchVertex vertex = vertices[node.index];
    PatchData patch = patches[draw.slot];

    // Two numbers from one sample: `x` -- height in storage units, `y` --
    // slope in its own (`tiles::SLOPE_UNIT`).
    float2 tile = sample_tile(patch, node.grid);
    float height = tile.x;
    float slope = tile.y * uniforms.detail.y;

    // The distance to the camera is taken at the UNDISPLACED vertex, and that
    // is not an oversight: `origin` is already camera-relative, so the length
    // of the sum is the distance. Computing it after the displacement would
    // make the detail depend on itself and would gain nothing -- the detail is
    // metres against hundreds.
    //
    // Per vertex, not per patch: `length(origin)` would be the same for the
    // whole patch, i.e. the octave weight would become a function of the patch
    // rather than of position -- and two neighbours of different levels would
    // diverge at a shared node (rule 3).
    float3 base_world = patch.origin + mul(uniforms.model, float4(vertex.offset, 0.0)).xyz;
    float distance = length(base_world);

    float2 detail = detail_sample(vertex.normal, slope, distance);

    // Both heights displace the vertex along the patch normal -- on the UNIT
    // sphere, because the radius and rotation are applied by the same matrix
    // as to the offset itself. The `terrain.x` factor is `scale_m /
    // radius_m`; the detail arrives in metres already, so it is divided by the
    // radius.
    float lift = height * uniforms.terrain.x + detail.x / uniforms.detail.x;
    float3 offset = vertex.offset + vertex.normal * lift;
    VertexOutput output = place(draw.slot, vertex, node.grid, offset);
    // The same two numbers that just displaced the vertex -- the rule reads no
    // others. So the colour cannot diverge from the shape: it is derived from
    // it.
    output.tint = material_tint(slope, detail.y, height);
    return output;
}

// The unit facet normal, turned outward.
//
// The derivatives give a vector of arbitrary sign -- the sphere normal, whose
// direction is always right, turns it. A degenerate product (a triangle
// collapsed to a point on screen) returns the sphere normal rather than NaN:
// there is no need to divide by zero here, and `normalize(0)` would reach the
// frame as a black dot.
float3 outward(float3 cross_product, float3 sphere)
{
    float length_squared = dot(cross_product, cross_product);
    if (length_squared < 1e-30)
    {
        return sphere;
    }
    float3 unit = cross_product * rsqrt(length_squared);
    return dot(unit, sphere) < 0.0 ? -unit : unit;
}

float3 shade(float3 normal, float3 albedo)
{
    float3 n = normalize(normal);
    float diffuse = max(dot(n, uniforms.light_dir.xyz), 0.0);
    return albedo * (0.05 + 0.95 * diffuse);
}

// The colour at a point of the colour tile, bilinear between four texels
// (T3b).
//
// The same shape as `sample_tile` and for the same reason: the window has
// already turned the patch node into a texel of **this** tile, and the
// interpolation between texels has to be done by hand -- a sampler knows
// nothing of the window.
//
// Clamping on both axes to `[0, NODES - 1]`: at a node on the very edge of the
// patch the right neighbour is already outside the texture, and its weight
// there is zero.
float3 sample_colour(uint tile, float2 node)
{
    float top = float(NODES - 1u);
    float x = clamp(node.x, 0.0, top);
    float y = clamp(node.y, 0.0, top);

    int xi = int(floor(x));
    int yi = int(floor(y));
    float tx = x - float(xi);
    float ty = y - float(yi);
    int xi1 = min(xi + 1, int(top));
    int yi1 = min(yi + 1, int(top));

    // `a` is the tile row, i.e. the texture's `y`: the same order as in
    // `sample_tile`, and swapping it here would transpose the colour relative
    // to the terrain.
    float3 c00 = colours[tile].Load(int3(yi, xi, 0)).rgb;
    float3 c01 = colours[tile].Load(int3(yi1, xi, 0)).rgb;
    float3 c10 = colours[tile].Load(int3(yi, xi1, 0)).rgb;
    float3 c11 = colours[tile].Load(int3(yi1, xi1, 0)).rgb;

    float3 left = c00 * (1.0 - ty) + c01 * ty;
    float3 right = c10 * (1.0 - ty) + c11 * ty;
    return left * (1.0 - tx) + right * tx;
}

// The surface colour at this pixel: the tile if there is one, otherwise the
// body colour.
//
// A branch on a uniform, and in the **fragment** stage, where it is safe: the
// ACO trap of F6 concerned the vertex stage (the same uniform was read wrong
// there), while the fragment read it correctly -- verified by writing the
// value into the colour. A second entry point is not needed here: a smooth
// body is drawn by its own pipeline and never enters this function.
//
// The material factor is applied in **both** branches, deliberately: the rule
// describes the surface, not the presence of a mosaic. A body with terrain but
// without colour gets the same relief contrast -- otherwise the arrival of an
// asset would change not only the tint but the very presence of detail.
float3 surface_albedo(VertexOutput input)
{
    if (uniforms.terrain.y <= 0.0)
    {
        return uniforms.colour.rgb * input.tint;
    }
    float3 unit = sample_colour(input.colour_tile, input.colour_node);
    // A single-channel tileset (the Moon) carries brightness, and exactly the
    // first channel is read from it -- the rest are zeros in such a texture. A
    // four-channel one (Earth) carries colour; the hardware decodes sRGB when
    // reading the texel, so it arrives here already linear.
    float3 albedo = uniforms.terrain.z >= 2.0 ? unit : unit.xxx;
    return albedo * uniforms.terrain.y * input.tint;
}

[shader("fragment")]
float4 fragment_smooth(VertexOutput input) : SV_Target
{
    return float4(shade(input.normal, uniforms.colour.rgb), 1.0);
}

[shader("fragment")]
float4 fragment_terrain(VertexOutput input) : SV_Target
{
    // The geometric facet normal: terrain is a set of planes the size of a
    // DEM cell, and the analytic sphere normal knows nothing about them.
    //
    // WARNING: but the facet itself knows too much about the sphere. The
    //   derivatives are taken over the triangle, so the facet spans the whole
    //   triangle -- including the sphere's curvature, which the tessellation
    //   also breaks into planes. From afar, when a grid cell is larger than the
    //   terrain under it, exactly that break is what remains in the frame:
    //   **measured at T7f** -- Earth from 1e6 m came out striped in 135-pixel
    //   tiles with a 3% brightness jump, and the stripes followed the triangles
    //   rather than the patches.
    //
    // So the facet of **the same** grid before displacement is subtracted from
    // the facet: the tessellation break is identical in both and cancels,
    // leaving the pure terrain slope added to the analytic sphere normal. On a
    // smooth body the difference is identically zero, i.e. the rule does
    // nothing -- the same principle as the material factor.
    //
    // WARNING: the sphere normal is **normalised here**, and that is not
    //   cosmetics: `model` is the rotation times the body radius, so
    //   `input.normal` arrives as long as the radius. A correction of length
    //   0.05 added to a vector of length 1.7e6 vanishes without a trace -- which
    //   is exactly how this fix, on its first attempt, switched the terrain off
    //   entirely, matching the frame with a smooth sphere to within one unit.
    float3 sphere = normalize(input.normal);
    float3 facet = outward(cross(ddx(input.world), ddy(input.world)), sphere);
    float3 tessellation = outward(cross(ddx(input.base), ddy(input.base)), sphere);
    float3 normal = normalize(sphere + facet - tessellation);
    return float4(shade(normal, surface_albedo(input)), 1.0);
}
