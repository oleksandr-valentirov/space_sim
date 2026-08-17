// Patch culling in compute (ROADMAP-PLANETS.md, R6b).
//
// The same thing `engine::cull` does on the CPU, and deliberately by the same
// formulas: the step's oracle is that both independent paths keep the same
// number of patches, and that agreement must be exact rather than "within
// tolerance". So every line below has a twin in Rust, and a divergence
// between them is a bug rather than a property of the hardware.
//
// ## What stays on the CPU
//
// Level selection, balancing and the stitching masks: they build the set, and
// the set is needed whole before culling (see `cull.rs` -- the mask is
// computed from what is in the set, so neighbours cannot be discarded during
// the traversal). A ready list of candidates arrives here and a subset of it
// leaves.
//
// ## The order of survivors is non-deterministic, deliberately
//
// Compaction goes through an atomic counter, so the order depends on which
// groups finished first. The frame does not care -- patches are opaque and
// ordered by depth -- and the comparison with the CPU is of the **count**,
// not the sequence. Written out loud, because in this project
// non-determinism is usually a bug, and a silent exception would be worse
// than the exception itself.

// A candidate: the geometry slot in the cache and the stitching mask. The
// same pair the vertex stage reads -- not a coincidence but a requirement:
// compute writes exactly what it will later read.
struct PatchDraw
{
    uint slot;
    uint mask;
};

// The patch's cone on the unit sphere, in the BODY frame.
struct Cone
{
    float3 axis;
    float cos_half;
    float sin_half;
};

struct CullParams
{
    // xyz -- direction to the camera in body frame, w -- cosine of the limb.
    float4 to_eye;
    // Rows of the view rotation: world axes -> camera axes.
    float4 view_right;
    float4 view_up;
    float4 view_back;
    // x, y -- tangents of the frame half-angles; z, w -- plane normalisers.
    float4 frustum;
    // x -- how many candidates, the rest zeros.
    uint4 counts;
    // x -- the body radius in metres.
    float4 body;
};

ConstantBuffer<CullParams> params;

StructuredBuffer<PatchDraw> candidates;
StructuredBuffer<Cone> cones;
// The patch origin relative to the camera, in world axes -- also the centre
// of the bounding sphere. The same buffer the vertex stage reads.
//
// WARNING: the struct must match `PatchData` in `patch.slang` **field for
//   field**: it is one buffer with one stride. There is no shared declaration
//   between the two shaders, so there are two copies -- exactly like `SIDE`
//   and `node_of`, with the same conclusion: both must be edited.
//
// A divergence here is not silent:
// `gpu_driven::the_gpu_draws_exactly_as_many_patches_as_the_cpu_kept` fails on
// the very first frame, because a shifted stride gives someone else's centre
// and with it someone else's culling. That is how it was found at R7a.
struct PatchData
{
    float3 origin;
    uint tile;
    float2 window_origin;
    float window_step;
    // Padding: `window_delta` lived here before stage W. The word stayed,
    // because std430 requires eight-byte alignment before `colour_origin`
    // anyway.
    float pad;
    // The colour window (T3b). Culling does not read it -- it is here purely
    // for the buffer **stride**, which is why the field order must match
    // `patch.slang`: `float2` after the scalars, or std430 adds a hole.
    uint colour_tile;
    float colour_step;
    float2 colour_origin;
};

StructuredBuffer<PatchData> origins;

RWStructuredBuffer<PatchDraw> visible;
// The `draw_indirect` arguments: [vertex_count, instance_count, first_vertex,
// first_instance]. The second element is the survivor counter.
RWStructuredBuffer<Atomic<uint>> indirect;

[shader("compute")]
[numthreads(64, 1, 1)]
void cull_main(uint3 id : SV_DispatchThreadID)
{
    uint index = id.x;
    if (index >= params.counts.x)
    {
        return;
    }

    PatchDraw candidate = candidates[index];
    Cone cone = cones[candidate.slot];

    // The horizon: the best a patch can do is cos(beta - alpha), expanded to
    // cos beta cos alpha + sin beta sin alpha. Verbatim `cull::beyond_limb`,
    // including the "eye inside the cone" special case: the cosine is even
    // there and the formula would return the angle to the EDGE of the cap
    // instead of zero, discarding the patch the camera stands over. Both
    // copies must be edited together.
    float cos_beta = clamp(dot(cone.axis, params.to_eye.xyz), -1.0, 1.0);
    if (cos_beta < cone.cos_half)
    {
        float sin_beta = sqrt(max(1.0 - cos_beta * cos_beta, 0.0));
        float best = cos_beta * cone.cos_half + sin_beta * cone.sin_half;
        if (best <= params.to_eye.w)
        {
            return;
        }
    }

    // Frustum: the patch's bounding sphere against the four side planes. The
    // centre is the patch origin (it lies on the cone axis), the radius is the
    // chord to the furthest node. Verbatim `cull::bounding_sphere` and
    // `cull::frustum`.
    float3 centre = origins[candidate.slot].origin;
    float radius = params.body.x * sqrt(max(2.0 - 2.0 * cone.cos_half, 0.0));

    float3 view = float3(
        dot(centre, params.view_right.xyz),
        dot(centre, params.view_up.xyz),
        dot(centre, params.view_back.xyz));

    float tx = params.frustum.x;
    float ty = params.frustum.y;
    bool outside =
        (view.x + tx * view.z) * params.frustum.z > radius ||
        (-view.x + tx * view.z) * params.frustum.z > radius ||
        (view.y + ty * view.z) * params.frustum.w > radius ||
        (-view.y + ty * view.z) * params.frustum.w > radius;
    if (outside)
    {
        return;
    }

    // `Atomic<uint>` rather than a plain `uint`: in WGSL atomicity is a
    // property of the TYPE, and without it Slang simply will not compile this
    // entry point for this target.
    uint slot = indirect[1].add(1u);
    visible[slot] = candidate;
}
