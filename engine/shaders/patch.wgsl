enable wgpu_binding_array;

struct PatchDraw_std430_0
{
    @align(4) slot_0 : u32,
    @align(4) mask_0 : u32,
};

@binding(3) @group(0) var<storage, read> draws_0 : array<PatchDraw_std430_0>;

struct PatchVertex_std430_0
{
    @align(16) offset_0 : vec3<f32>,
    @align(16) normal_0 : vec3<f32>,
};

@binding(2) @group(0) var<storage, read> vertices_0 : array<PatchVertex_std430_0>;

struct PatchData_std430_0
{
    @align(16) origin_0 : vec3<f32>,
    @align(4) tile_0 : u32,
    @align(16) window_origin_0 : vec2<f32>,
    @align(8) window_step_0 : f32,
    @align(4) pad_0 : f32,
    @align(16) colour_tile_0 : u32,
    @align(4) colour_step_0 : f32,
    @align(8) colour_origin_0 : vec2<f32>,
};

@binding(1) @group(0) var<storage, read> patches_0 : array<PatchData_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct Uniforms_std140_0
{
    @align(16) projection_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) model_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) light_dir_0 : vec4<f32>,
    @align(16) colour_0 : vec4<f32>,
    @align(16) terrain_0 : vec4<f32>,
    @align(16) detail_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> uniforms_0 : Uniforms_std140_0;
@binding(0) @group(1) var tiles_0 : binding_array<texture_2d<i32>>;

@binding(1) @group(1) var colours_0 : binding_array<texture_2d<f32>>;

fn rsqrt_0( x_0 : f32) -> f32
{
    return 1.0f / sqrt(x_0);
}

struct Node_0
{
     grid_0 : vec2<u32>,
     index_0 : u32,
};

fn node_of_0( vertex_0 : u32,  slot_1 : u32,  mask_1 : u32) -> Node_0
{
    var triangle_0 : u32 = vertex_0 / u32(3);
    var corner_0 : u32 = vertex_0 % u32(3);
    var cell_0 : u32 = triangle_0 / u32(2);
    var a_0 : u32 = cell_0 / u32(32);
    var b_0 : u32 = cell_0 % u32(32);
    const _S1 : vec2<u32> = vec2<u32>(u32(1), u32(0));
    const _S2 : vec2<u32> = vec2<u32>(u32(0), u32(1));
    var first_0 : array<vec2<u32>, i32(3)> = array<vec2<u32>, i32(3)>( vec2<u32>(u32(0), u32(0)), _S1, _S2 );
    var second_0 : array<vec2<u32>, i32(3)> = array<vec2<u32>, i32(3)>( _S2, _S1, vec2<u32>(u32(1), u32(1)) );
    var step_0 : vec2<u32>;
    if((triangle_0 % u32(2)) == u32(0))
    {
        step_0 = first_0[corner_0];
    }
    else
    {
        step_0 = second_0[corner_0];
    }
    var a_1 : u32 = a_0 + step_0.x;
    var b_1 : u32 = b_0 + step_0.y;
    var odd_on_b_0 : bool;
    if((a_1 % u32(2)) == u32(1))
    {
        if(b_1 == u32(0))
        {
            odd_on_b_0 = ((mask_1 & (u32(4)))) != u32(0);
        }
        else
        {
            odd_on_b_0 = false;
        }
        if(odd_on_b_0)
        {
            odd_on_b_0 = true;
        }
        else
        {
            if(b_1 == u32(32))
            {
                odd_on_b_0 = ((mask_1 & (u32(8)))) != u32(0);
            }
            else
            {
                odd_on_b_0 = false;
            }
        }
    }
    else
    {
        odd_on_b_0 = false;
    }
    var odd_on_a_0 : bool;
    if((b_1 % u32(2)) == u32(1))
    {
        if(a_1 == u32(0))
        {
            odd_on_a_0 = ((mask_1 & (u32(1)))) != u32(0);
        }
        else
        {
            odd_on_a_0 = false;
        }
        if(odd_on_a_0)
        {
            odd_on_a_0 = true;
        }
        else
        {
            if(a_1 == u32(32))
            {
                odd_on_a_0 = ((mask_1 & (u32(2)))) != u32(0);
            }
            else
            {
                odd_on_a_0 = false;
            }
        }
    }
    else
    {
        odd_on_a_0 = false;
    }
    var a_2 : u32;
    if(odd_on_b_0)
    {
        a_2 = a_1 - u32(1);
    }
    else
    {
        a_2 = a_1;
    }
    var b_2 : u32;
    if(odd_on_a_0)
    {
        b_2 = b_1 - u32(1);
    }
    else
    {
        b_2 = b_1;
    }
    var out_0 : Node_0;
    out_0.grid_0 = vec2<u32>(a_2, b_2);
    out_0.index_0 = slot_1 * u32(33) * u32(33) + a_2 * u32(33) + b_2;
    return out_0;
}

struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) normal_1 : vec3<f32>,
    @location(1) world_0 : vec3<f32>,
    @location(2) base_0 : vec3<f32>,
    @interpolate(flat) @location(3) colour_tile_1 : u32,
    @location(4) colour_node_0 : vec2<f32>,
    @location(5) tint_0 : f32,
};

fn place_0( slot_2 : u32,  vertex_1 : ptr<function, PatchVertex_std430_0>,  grid_1 : vec2<u32>,  offset_1 : vec3<f32>) -> VertexOutput_0
{
    var patch_0 : PatchData_std430_0 = patches_0[slot_2];
    var world_1 : vec3<f32> = patch_0.origin_0 + (((vec4<f32>(offset_1, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    var output_0 : VertexOutput_0;
    output_0.tint_0 = 1.0f;
    output_0.colour_tile_1 = patch_0.colour_tile_0;
    output_0.colour_node_0 = patch_0.colour_origin_0 + vec2<f32>(grid_1) * vec2<f32>(patch_0.colour_step_0);
    output_0.position_0 = (((vec4<f32>(world_1, 1.0f)) * (mat4x4<f32>(uniforms_0.projection_0.data_0[i32(0)][i32(0)], uniforms_0.projection_0.data_0[i32(1)][i32(0)], uniforms_0.projection_0.data_0[i32(2)][i32(0)], uniforms_0.projection_0.data_0[i32(3)][i32(0)], uniforms_0.projection_0.data_0[i32(0)][i32(1)], uniforms_0.projection_0.data_0[i32(1)][i32(1)], uniforms_0.projection_0.data_0[i32(2)][i32(1)], uniforms_0.projection_0.data_0[i32(3)][i32(1)], uniforms_0.projection_0.data_0[i32(0)][i32(2)], uniforms_0.projection_0.data_0[i32(1)][i32(2)], uniforms_0.projection_0.data_0[i32(2)][i32(2)], uniforms_0.projection_0.data_0[i32(3)][i32(2)], uniforms_0.projection_0.data_0[i32(0)][i32(3)], uniforms_0.projection_0.data_0[i32(1)][i32(3)], uniforms_0.projection_0.data_0[i32(2)][i32(3)], uniforms_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_0.normal_1 = (((vec4<f32>((*vertex_1).normal_0, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    output_0.world_0 = world_1;
    output_0.base_0 = patch_0.origin_0 + (((vec4<f32>((*vertex_1).offset_0, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    return output_0;
}

@vertex
fn vertex_smooth(@builtin(vertex_index) vertex_2 : u32, @builtin(instance_index) instance_0 : u32) -> VertexOutput_0
{
    var draw_0 : PatchDraw_std430_0 = draws_0[instance_0];
    var node_0 : Node_0 = node_of_0(vertex_2, draw_0.slot_0, draw_0.mask_0);
    var _S3 : PatchVertex_std430_0 = vertices_0[node_0.index_0];
    var _S4 : VertexOutput_0 = place_0(draw_0.slot_0, &(_S3), node_0.grid_0, _S3.offset_0);
    return _S4;
}

fn sample_tile_0( patch_1 : ptr<function, PatchData_std430_0>,  grid_2 : vec2<u32>) -> vec2<f32>
{
    var x_1 : f32 = (*patch_1).window_origin_0.x + f32(grid_2.x) * (*patch_1).window_step_0;
    var y_0 : f32 = (*patch_1).window_origin_0.y + f32(grid_2.y) * (*patch_1).window_step_0;
    var x0_0 : f32 = floor(x_1);
    var y0_0 : f32 = floor(y_0);
    var tx_0 : f32 = x_1 - x0_0;
    var ty_0 : f32 = y_0 - y0_0;
    var xi_0 : i32 = i32(x0_0);
    var yi_0 : i32 = i32(y0_0);
    var _S5 : i32 = min(xi_0 + i32(1), i32(32));
    var _S6 : i32 = min(yi_0 + i32(1), i32(32));
    var _S7 : vec3<i32> = vec3<i32>(yi_0, xi_0, i32(0));
    var _S8 : vec3<i32> = vec3<i32>(_S6, xi_0, i32(0));
    var _S9 : vec3<i32> = vec3<i32>(yi_0, _S5, i32(0));
    var _S10 : vec3<i32> = vec3<i32>(_S6, _S5, i32(0));
    var _S11 : vec2<f32> = vec2<f32>((1.0f - ty_0));
    var _S12 : vec2<f32> = vec2<f32>(ty_0);
    return (vec2<f32>((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S7)).xy, ((_S7)).z).xy).xy) * _S11 + vec2<f32>((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S8)).xy, ((_S8)).z).xy).xy) * _S12) * vec2<f32>((1.0f - tx_0)) + (vec2<f32>((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S9)).xy, ((_S9)).z).xy).xy) * _S11 + vec2<f32>((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S10)).xy, ((_S10)).z).xy).xy) * _S12) * vec2<f32>(tx_0);
}

fn detail_smooth_0( t_0 : f32) -> f32
{
    return t_0 * t_0 * (3.0f - 2.0f * t_0);
}

fn octave_weight_0( wavelength_0 : f32,  distance_0 : f32,  focal_0 : f32) -> f32
{
    var px_0 : f32 = wavelength_0 / max(distance_0, 1.0f) * focal_0;
    if(px_0 <= 4.0f)
    {
        return 0.0f;
    }
    if(px_0 >= 16.0f)
    {
        return 1.0f;
    }
    return detail_smooth_0((px_0 - 4.0f) / 12.0f);
}

fn detail_hash_0( x_2 : i32,  y_1 : i32,  z_0 : i32) -> f32
{
    var h_0 : u32 = ((((u32(x_2) * u32(2654435761)) ^ ((u32(y_1) * u32(2246822507))))) ^ ((u32(z_0) * u32(3266489909))));
    var h_1 : u32 = ((h_0 ^ (((h_0 >> (u32(15))))))) * u32(625341585);
    var h_2 : u32 = ((h_1 ^ (((h_1 >> (u32(13))))))) * u32(668265263);
    return f32((((h_2 ^ (((h_2 >> (u32(16))))))) >> (u32(8)))) / 1.6777216e+07f;
}

fn value_noise_0( p_0 : vec3<f32>) -> f32
{
    var cell_1 : vec3<f32> = floor(p_0);
    var _S13 : f32 = cell_1.x;
    var _S14 : f32 = detail_smooth_0(p_0.x - _S13);
    var _S15 : f32 = cell_1.y;
    var _S16 : f32 = detail_smooth_0(p_0.y - _S15);
    var _S17 : f32 = cell_1.z;
    var _S18 : f32 = detail_smooth_0(p_0.z - _S17);
    var _S19 : i32 = i32(_S13);
    var _S20 : i32 = i32(_S15);
    var _S21 : i32 = i32(_S17);
    var dx_0 : i32 = i32(0);
    var out_1 : f32 = 0.0f;
    for(;;)
    {
        if(dx_0 < i32(2))
        {
        }
        else
        {
            break;
        }
        var _S22 : f32;
        if(dx_0 == i32(0))
        {
            _S22 = 1.0f - _S14;
        }
        else
        {
            _S22 = _S14;
        }
        var dy_0 : i32 = i32(0);
        for(;;)
        {
            if(dy_0 < i32(2))
            {
            }
            else
            {
                break;
            }
            var _S23 : f32;
            if(dy_0 == i32(0))
            {
                _S23 = 1.0f - _S16;
            }
            else
            {
                _S23 = _S16;
            }
            var dz_0 : i32 = i32(0);
            var out_2 : f32 = out_1;
            for(;;)
            {
                if(dz_0 < i32(2))
                {
                }
                else
                {
                    break;
                }
                var wz_0 : f32;
                if(dz_0 == i32(0))
                {
                    wz_0 = 1.0f - _S18;
                }
                else
                {
                    wz_0 = _S18;
                }
                var out_3 : f32 = out_2 + detail_hash_0(_S19 + dx_0, _S20 + dy_0, _S21 + dz_0) * _S22 * _S23 * wz_0;
                dz_0 = dz_0 + i32(1);
                out_2 = out_3;
            }
            dy_0 = dy_0 + i32(1);
            out_1 = out_2;
        }
        dx_0 = dx_0 + i32(1);
    }
    return out_1;
}

fn detail_sample_0( unit_0 : vec3<f32>,  slope_0 : f32,  distance_1 : f32) -> vec2<f32>
{
    var _S24 : f32 = uniforms_0.detail_0.x;
    var _S25 : f32 = uniforms_0.detail_0.z;
    var _S26 : f32 = uniforms_0.detail_0.w;
    var octave_0 : u32 = u32(0);
    var height_0 : f32 = 0.0f;
    var roughness_0 : f32 = 0.0f;
    for(;;)
    {
        if(octave_0 < u32(6))
        {
        }
        else
        {
            break;
        }
        var wavelength_1 : f32 = _S25 / f32((u32(1) << (octave_0)));
        var weight_0 : f32 = octave_weight_0(wavelength_1, distance_1, _S26);
        if(weight_0 <= 0.0f)
        {
            break;
        }
        var signed_noise_0 : f32 = value_noise_0(unit_0 * vec3<f32>((_S24 / wavelength_1))) - 0.5f;
        var height_1 : f32 = height_0 + signed_noise_0 * 0.5f * slope_0 * wavelength_1 * weight_0;
        var roughness_1 : f32 = roughness_0 + 2.0f * signed_noise_0 * weight_0;
        octave_0 = octave_0 + u32(1);
        height_0 = height_1;
        roughness_0 = roughness_1;
    }
    return vec2<f32>(height_0, roughness_0);
}

fn material_tint_0( slope_1 : f32,  roughness_2 : f32,  height_2 : f32) -> f32
{
    if(height_2 < (uniforms_0.terrain_0.w))
    {
        return 1.0f;
    }
    return clamp(1.0f + clamp(slope_1 / 0.15000000596046448f, 0.0f, 1.0f) * (0.30000001192092896f + 0.44999998807907104f * roughness_2), 0.34999999403953552f, 1.79999995231628418f);
}

@vertex
fn vertex_terrain(@builtin(vertex_index) vertex_3 : u32, @builtin(instance_index) instance_1 : u32) -> VertexOutput_0
{
    var draw_1 : PatchDraw_std430_0 = draws_0[instance_1];
    var node_1 : Node_0 = node_of_0(vertex_3, draw_1.slot_0, draw_1.mask_0);
    var _S27 : PatchVertex_std430_0 = vertices_0[node_1.index_0];
    var _S28 : PatchData_std430_0 = patches_0[draw_1.slot_0];
    var _S29 : vec2<f32> = sample_tile_0(&(_S28), node_1.grid_0);
    var height_3 : f32 = _S29.x;
    var slope_2 : f32 = _S29.y * uniforms_0.detail_0.y;
    var detail_1 : vec2<f32> = detail_sample_0(_S27.normal_0, slope_2, length(_S28.origin_0 + (((vec4<f32>(_S27.offset_0, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz));
    var _S30 : VertexOutput_0 = place_0(draw_1.slot_0, &(_S27), node_1.grid_0, _S27.offset_0 + _S27.normal_0 * vec3<f32>((height_3 * uniforms_0.terrain_0.x + detail_1.x / uniforms_0.detail_0.x)));
    var output_1 : VertexOutput_0 = _S30;
    output_1.tint_0 = material_tint_0(slope_2, detail_1.y, height_3);
    return output_1;
}

fn shade_0( normal_2 : vec3<f32>,  albedo_0 : vec3<f32>) -> vec3<f32>
{
    return albedo_0 * vec3<f32>((0.05000000074505806f + 0.94999998807907104f * max(dot(normalize(normal_2), uniforms_0.light_dir_0.xyz), 0.0f)));
}

struct pixelOutput_0
{
    @location(0) output_2 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) normal_3 : vec3<f32>,
    @location(1) world_2 : vec3<f32>,
    @location(2) base_1 : vec3<f32>,
    @interpolate(flat) @location(3) colour_tile_2 : u32,
    @location(4) colour_node_1 : vec2<f32>,
    @location(5) tint_1 : f32,
};

@fragment
fn fragment_smooth( _S31 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S32 : pixelOutput_0 = pixelOutput_0( vec4<f32>(shade_0(_S31.normal_3, uniforms_0.colour_0.xyz), 1.0f) );
    return _S32;
}

fn outward_0( cross_product_0 : vec3<f32>,  sphere_0 : vec3<f32>) -> vec3<f32>
{
    var length_squared_0 : f32 = dot(cross_product_0, cross_product_0);
    if(length_squared_0 < 1.00000000317107685e-30f)
    {
        return sphere_0;
    }
    var unit_1 : vec3<f32> = cross_product_0 * vec3<f32>(rsqrt_0(length_squared_0));
    var _S33 : vec3<f32>;
    if((dot(unit_1, sphere_0)) < 0.0f)
    {
        _S33 = (vec3<f32>(0) - unit_1);
    }
    else
    {
        _S33 = unit_1;
    }
    return _S33;
}

fn sample_colour_0( tile_1 : u32,  node_2 : vec2<f32>) -> vec3<f32>
{
    var x_3 : f32 = clamp(node_2.x, 0.0f, 32.0f);
    var y_2 : f32 = clamp(node_2.y, 0.0f, 32.0f);
    var xi_1 : i32 = i32(floor(x_3));
    var yi_1 : i32 = i32(floor(y_2));
    var tx_1 : f32 = x_3 - f32(xi_1);
    var ty_1 : f32 = y_2 - f32(yi_1);
    var _S34 : i32 = min(xi_1 + i32(1), i32(32));
    var _S35 : i32 = min(yi_1 + i32(1), i32(32));
    var _S36 : vec3<i32> = vec3<i32>(yi_1, xi_1, i32(0));
    var _S37 : vec3<i32> = vec3<i32>(_S35, xi_1, i32(0));
    var _S38 : vec3<i32> = vec3<i32>(yi_1, _S34, i32(0));
    var _S39 : vec3<i32> = vec3<i32>(_S35, _S34, i32(0));
    var _S40 : vec3<f32> = vec3<f32>((1.0f - ty_1));
    var _S41 : vec3<f32> = vec3<f32>(ty_1);
    return ((textureLoad((colours_0[tile_1]), ((_S36)).xy, ((_S36)).z)).xyz * _S40 + (textureLoad((colours_0[tile_1]), ((_S37)).xy, ((_S37)).z)).xyz * _S41) * vec3<f32>((1.0f - tx_1)) + ((textureLoad((colours_0[tile_1]), ((_S38)).xy, ((_S38)).z)).xyz * _S40 + (textureLoad((colours_0[tile_1]), ((_S39)).xy, ((_S39)).z)).xyz * _S41) * vec3<f32>(tx_1);
}

fn surface_albedo_0( input_0 : VertexOutput_0) -> vec3<f32>
{
    if((uniforms_0.terrain_0.y) <= 0.0f)
    {
        return uniforms_0.colour_0.xyz * vec3<f32>(input_0.tint_0);
    }
    var unit_2 : vec3<f32> = sample_colour_0(input_0.colour_tile_1, input_0.colour_node_0);
    var albedo_1 : vec3<f32>;
    if((uniforms_0.terrain_0.z) >= 2.0f)
    {
        albedo_1 = unit_2;
    }
    else
    {
        albedo_1 = unit_2.xxx;
    }
    return albedo_1 * vec3<f32>(uniforms_0.terrain_0.y) * vec3<f32>(input_0.tint_0);
}

struct pixelOutput_1
{
    @location(0) output_3 : vec4<f32>,
};

struct pixelInput_1
{
    @location(0) normal_4 : vec3<f32>,
    @location(1) world_3 : vec3<f32>,
    @location(2) base_2 : vec3<f32>,
    @interpolate(flat) @location(3) colour_tile_3 : u32,
    @location(4) colour_node_2 : vec2<f32>,
    @location(5) tint_2 : f32,
};

@fragment
fn fragment_terrain( _S42 : pixelInput_1, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_1
{
    var _S43 : VertexOutput_0 = VertexOutput_0( position_2, _S42.normal_4, _S42.world_3, _S42.base_2, _S42.colour_tile_3, _S42.colour_node_2, _S42.tint_2 );
    var sphere_1 : vec3<f32> = normalize(_S42.normal_4);
    var _S44 : pixelOutput_1 = pixelOutput_1( vec4<f32>(shade_0(normalize(sphere_1 + outward_0(cross(dpdx(_S42.world_3), dpdy(_S42.world_3)), sphere_1) - outward_0(cross(dpdx(_S42.base_2), dpdy(_S42.base_2)), sphere_1)), surface_albedo_0(_S43)), 1.0f) );
    return _S44;
}