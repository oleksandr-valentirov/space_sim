struct CullParams_std140_0
{
    @align(16) to_eye_0 : vec4<f32>,
    @align(16) view_right_0 : vec4<f32>,
    @align(16) view_up_0 : vec4<f32>,
    @align(16) view_back_0 : vec4<f32>,
    @align(16) frustum_0 : vec4<f32>,
    @align(16) counts_0 : vec4<u32>,
    @align(16) body_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> params_0 : CullParams_std140_0;
struct PatchDraw_std430_0
{
    @align(4) slot_0 : u32,
    @align(4) mask_0 : u32,
};

@binding(1) @group(0) var<storage, read> candidates_0 : array<PatchDraw_std430_0>;

struct Cone_std430_0
{
    @align(16) axis_0 : vec3<f32>,
    @align(4) cos_half_0 : f32,
    @align(16) sin_half_0 : f32,
};

@binding(2) @group(0) var<storage, read> cones_0 : array<Cone_std430_0>;

struct PatchData_std430_0
{
    @align(16) origin_0 : vec3<f32>,
    @align(4) tile_0 : u32,
    @align(16) window_origin_0 : vec2<f32>,
    @align(8) window_step_0 : f32,
    @align(4) _pad_0 : f32,
};

@binding(3) @group(0) var<storage, read> origins_0 : array<PatchData_std430_0>;

@binding(5) @group(0) var<storage, read_write> indirect_0 : array<atomic<u32>>;

@binding(4) @group(0) var<storage, read_write> visible_0 : array<PatchDraw_std430_0>;

@compute
@workgroup_size(64, 1, 1)
fn cull_main(@builtin(global_invocation_id) id_0 : vec3<u32>)
{
    var index_0 : u32 = id_0.x;
    if(index_0 >= (params_0.counts_0.x))
    {
        return;
    }
    var candidate_0 : PatchDraw_std430_0 = candidates_0[index_0];
    var cone_0 : Cone_std430_0 = cones_0[candidate_0.slot_0];
    var cos_beta_0 : f32 = clamp(dot(cone_0.axis_0, params_0.to_eye_0.xyz), -1.0f, 1.0f);
    if(cos_beta_0 < (cone_0.cos_half_0))
    {
        if((cos_beta_0 * cone_0.cos_half_0 + sqrt(max(1.0f - cos_beta_0 * cos_beta_0, 0.0f)) * cone_0.sin_half_0) <= (params_0.to_eye_0.w))
        {
            return;
        }
    }
    var _S1 : PatchData_std430_0 = origins_0[candidate_0.slot_0];
    var radius_0 : f32 = params_0.body_0.x * sqrt(max(2.0f - 2.0f * cone_0.cos_half_0, 0.0f));
    var _S2 : f32 = dot(_S1.origin_0, params_0.view_right_0.xyz);
    var _S3 : f32 = dot(_S1.origin_0, params_0.view_up_0.xyz);
    var _S4 : f32 = dot(_S1.origin_0, params_0.view_back_0.xyz);
    var ty_0 : f32 = params_0.frustum_0.y;
    var _S5 : f32 = params_0.frustum_0.x * _S4;
    var outside_0 : bool;
    if(((_S2 + _S5) * params_0.frustum_0.z) > radius_0)
    {
        outside_0 = true;
    }
    else
    {
        outside_0 = ((- _S2 + _S5) * params_0.frustum_0.z) > radius_0;
    }
    if(outside_0)
    {
        outside_0 = true;
    }
    else
    {
        outside_0 = ((_S3 + ty_0 * _S4) * params_0.frustum_0.w) > radius_0;
    }
    if(outside_0)
    {
        outside_0 = true;
    }
    else
    {
        outside_0 = ((- _S3 + ty_0 * _S4) * params_0.frustum_0.w) > radius_0;
    }
    if(outside_0)
    {
        return;
    }
    var slot_1 : u32 = atomicAdd(&(indirect_0[i32(1)]), u32(1));
    visible_0[slot_1] = candidate_0;
    return;
}

