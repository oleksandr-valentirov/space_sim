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
    @align(4) window_delta_0 : f32,
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
    @interpolate(flat) @location(2) colour_tile_1 : u32,
    @location(3) colour_node_0 : vec2<f32>,
    @location(4) tint_0 : f32,
};

fn place_0( slot_2 : u32,  vertex_1 : ptr<function, PatchVertex_std430_0>,  grid_1 : vec2<u32>,  offset_1 : vec3<f32>) -> VertexOutput_0
{
    var patch_0 : PatchData_std430_0 = patches_0[slot_2];
    var world_1 : vec3<f32> = patch_0.origin_0 + (((vec4<f32>(offset_1, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    var output_0 : VertexOutput_0;
    output_0.tint_0 = 1.0f;
    output_0.colour_tile_1 = patch_0.colour_tile_0;
    output_0.colour_node_0 = patch_0.colour_origin_0 + vec2<f32>(grid_1) * vec2<f32>(patch_0.colour_step_0) + vec2<f32>(1.0f);
    output_0.position_0 = (((vec4<f32>(world_1, 1.0f)) * (mat4x4<f32>(uniforms_0.projection_0.data_0[i32(0)][i32(0)], uniforms_0.projection_0.data_0[i32(1)][i32(0)], uniforms_0.projection_0.data_0[i32(2)][i32(0)], uniforms_0.projection_0.data_0[i32(3)][i32(0)], uniforms_0.projection_0.data_0[i32(0)][i32(1)], uniforms_0.projection_0.data_0[i32(1)][i32(1)], uniforms_0.projection_0.data_0[i32(2)][i32(1)], uniforms_0.projection_0.data_0[i32(3)][i32(1)], uniforms_0.projection_0.data_0[i32(0)][i32(2)], uniforms_0.projection_0.data_0[i32(1)][i32(2)], uniforms_0.projection_0.data_0[i32(2)][i32(2)], uniforms_0.projection_0.data_0[i32(3)][i32(2)], uniforms_0.projection_0.data_0[i32(0)][i32(3)], uniforms_0.projection_0.data_0[i32(1)][i32(3)], uniforms_0.projection_0.data_0[i32(2)][i32(3)], uniforms_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_0.normal_1 = (((vec4<f32>((*vertex_1).normal_0, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    output_0.world_0 = world_1;
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

fn sample_height_0( patch_1 : ptr<function, PatchData_std430_0>,  grid_2 : vec2<u32>) -> f32
{
    var x_0 : f32 = (*patch_1).window_origin_0.x + f32(grid_2.x) * (*patch_1).window_step_0;
    var y_0 : f32 = (*patch_1).window_origin_0.y + f32(grid_2.y) * (*patch_1).window_step_0;
    var x0_0 : f32 = floor(x_0);
    var y0_0 : f32 = floor(y_0);
    var tx_0 : f32 = x_0 - x0_0;
    var ty_0 : f32 = y_0 - y0_0;
    var _S5 : i32 = i32(x0_0) + i32(1);
    var _S6 : i32 = i32(y0_0) + i32(1);
    var _S7 : vec3<i32> = vec3<i32>(_S6, _S5, i32(0));
    var _S8 : i32 = min(_S6, i32(32)) + i32(1);
    var _S9 : vec3<i32> = vec3<i32>(_S8, _S5, i32(0));
    var _S10 : i32 = min(_S5, i32(32)) + i32(1);
    var _S11 : vec3<i32> = vec3<i32>(_S6, _S10, i32(0));
    var _S12 : vec3<i32> = vec3<i32>(_S8, _S10, i32(0));
    var _S13 : f32 = 1.0f - ty_0;
    return (f32((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S7)).xy, ((_S7)).z).x)) * _S13 + f32((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S9)).xy, ((_S9)).z).x)) * ty_0) * (1.0f - tx_0) + (f32((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S11)).xy, ((_S11)).z).x)) * _S13 + f32((textureLoad((tiles_0[(*patch_1).tile_0]), ((_S12)).xy, ((_S12)).z).x)) * ty_0) * tx_0;
}

fn units_at_0( patch_2 : ptr<function, PatchData_std430_0>,  x_1 : f32,  y_1 : f32) -> f32
{
    var x0_1 : f32 = floor(x_1);
    var y0_1 : f32 = floor(y_1);
    var tx_1 : f32 = x_1 - x0_1;
    var ty_1 : f32 = y_1 - y0_1;
    var _S14 : i32 = i32(x0_1) + i32(1);
    var _S15 : i32 = i32(y0_1) + i32(1);
    var _S16 : vec3<i32> = vec3<i32>(_S15, _S14, i32(0));
    var _S17 : i32 = min(_S15, i32(33)) + i32(1);
    var _S18 : vec3<i32> = vec3<i32>(_S17, _S14, i32(0));
    var _S19 : i32 = min(_S14, i32(33)) + i32(1);
    var _S20 : vec3<i32> = vec3<i32>(_S15, _S19, i32(0));
    var _S21 : vec3<i32> = vec3<i32>(_S17, _S19, i32(0));
    var _S22 : f32 = 1.0f - ty_1;
    return (f32((textureLoad((tiles_0[(*patch_2).tile_0]), ((_S16)).xy, ((_S16)).z).x)) * _S22 + f32((textureLoad((tiles_0[(*patch_2).tile_0]), ((_S18)).xy, ((_S18)).z).x)) * ty_1) * (1.0f - tx_1) + (f32((textureLoad((tiles_0[(*patch_2).tile_0]), ((_S20)).xy, ((_S20)).z).x)) * _S22 + f32((textureLoad((tiles_0[(*patch_2).tile_0]), ((_S21)).xy, ((_S21)).z).x)) * ty_1) * tx_1;
}

fn sample_slope_0( patch_3 : ptr<function, PatchData_std430_0>,  grid_3 : vec2<u32>) -> f32
{
    var x_2 : f32 = (*patch_3).window_origin_0.x + f32(grid_3.x) * (*patch_3).window_step_0;
    var y_2 : f32 = (*patch_3).window_origin_0.y + f32(grid_3.y) * (*patch_3).window_step_0;
    var _S23 : f32 = (*patch_3).window_delta_0;
    var _S24 : f32 = units_at_0(&((*patch_3)), x_2 + (*patch_3).window_delta_0, y_2);
    var _S25 : f32 = units_at_0(&((*patch_3)), x_2 - _S23, y_2);
    var du_0 : f32 = _S24 - _S25;
    var _S26 : f32 = units_at_0(&((*patch_3)), x_2, y_2 + _S23);
    var _S27 : f32 = units_at_0(&((*patch_3)), x_2, y_2 - _S23);
    var dv_0 : f32 = _S26 - _S27;
    var rise_0 : f32 = uniforms_0.detail_0.y;
    return sqrt(du_0 * du_0 * rise_0 * rise_0 + dv_0 * dv_0 * rise_0 * rise_0);
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

fn detail_hash_0( x_3 : i32,  y_3 : i32,  z_0 : i32) -> f32
{
    var h_0 : u32 = ((((u32(x_3) * u32(2654435761)) ^ ((u32(y_3) * u32(2246822507))))) ^ ((u32(z_0) * u32(3266489909))));
    var h_1 : u32 = ((h_0 ^ (((h_0 >> (u32(15))))))) * u32(625341585);
    var h_2 : u32 = ((h_1 ^ (((h_1 >> (u32(13))))))) * u32(668265263);
    return f32((((h_2 ^ (((h_2 >> (u32(16))))))) >> (u32(8)))) / 1.6777216e+07f;
}

fn value_noise_0( p_0 : vec3<f32>) -> f32
{
    var cell_1 : vec3<f32> = floor(p_0);
    var _S28 : f32 = cell_1.x;
    var _S29 : f32 = detail_smooth_0(p_0.x - _S28);
    var _S30 : f32 = cell_1.y;
    var _S31 : f32 = detail_smooth_0(p_0.y - _S30);
    var _S32 : f32 = cell_1.z;
    var _S33 : f32 = detail_smooth_0(p_0.z - _S32);
    var _S34 : i32 = i32(_S28);
    var _S35 : i32 = i32(_S30);
    var _S36 : i32 = i32(_S32);
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
        var _S37 : f32;
        if(dx_0 == i32(0))
        {
            _S37 = 1.0f - _S29;
        }
        else
        {
            _S37 = _S29;
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
            var _S38 : f32;
            if(dy_0 == i32(0))
            {
                _S38 = 1.0f - _S31;
            }
            else
            {
                _S38 = _S31;
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
                    wz_0 = 1.0f - _S33;
                }
                else
                {
                    wz_0 = _S33;
                }
                var out_3 : f32 = out_2 + detail_hash_0(_S34 + dx_0, _S35 + dy_0, _S36 + dz_0) * _S37 * _S38 * wz_0;
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
    var _S39 : f32 = uniforms_0.detail_0.x;
    var _S40 : f32 = uniforms_0.detail_0.z;
    var _S41 : f32 = uniforms_0.detail_0.w;
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
        var wavelength_1 : f32 = _S40 / f32((u32(1) << (octave_0)));
        var weight_0 : f32 = octave_weight_0(wavelength_1, distance_1, _S41);
        if(weight_0 <= 0.0f)
        {
            break;
        }
        var signed_noise_0 : f32 = value_noise_0(unit_0 * vec3<f32>((_S39 / wavelength_1))) - 0.5f;
        var height_1 : f32 = height_0 + signed_noise_0 * 0.5f * slope_0 * wavelength_1 * weight_0;
        var roughness_1 : f32 = roughness_0 + 2.0f * signed_noise_0 * weight_0;
        octave_0 = octave_0 + u32(1);
        height_0 = height_1;
        roughness_0 = roughness_1;
    }
    return vec2<f32>(height_0, roughness_0);
}

fn material_tint_0( slope_1 : f32,  roughness_2 : f32) -> f32
{
    return clamp(1.0f + clamp(slope_1 / 0.15000000596046448f, 0.0f, 1.0f) * (0.30000001192092896f + 0.44999998807907104f * roughness_2), 0.34999999403953552f, 1.79999995231628418f);
}

@vertex
fn vertex_terrain(@builtin(vertex_index) vertex_3 : u32, @builtin(instance_index) instance_1 : u32) -> VertexOutput_0
{
    var draw_1 : PatchDraw_std430_0 = draws_0[instance_1];
    var node_1 : Node_0 = node_of_0(vertex_3, draw_1.slot_0, draw_1.mask_0);
    var _S42 : PatchVertex_std430_0 = vertices_0[node_1.index_0];
    var _S43 : PatchData_std430_0 = patches_0[draw_1.slot_0];
    var _S44 : f32 = sample_height_0(&(_S43), node_1.grid_0);
    var distance_2 : f32 = length(_S43.origin_0 + (((vec4<f32>(_S42.offset_0, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz);
    var _S45 : f32 = sample_slope_0(&(_S43), node_1.grid_0);
    var detail_1 : vec2<f32> = detail_sample_0(_S42.normal_0, _S45, distance_2);
    var _S46 : VertexOutput_0 = place_0(draw_1.slot_0, &(_S42), node_1.grid_0, _S42.offset_0 + _S42.normal_0 * vec3<f32>((_S44 * uniforms_0.terrain_0.x + detail_1.x / uniforms_0.detail_0.x)));
    var output_1 : VertexOutput_0 = _S46;
    output_1.tint_0 = material_tint_0(_S45, detail_1.y);
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
    @interpolate(flat) @location(2) colour_tile_2 : u32,
    @location(3) colour_node_1 : vec2<f32>,
    @location(4) tint_1 : f32,
};

@fragment
fn fragment_smooth( _S47 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S48 : pixelOutput_0 = pixelOutput_0( vec4<f32>(shade_0(_S47.normal_3, uniforms_0.colour_0.xyz), 1.0f) );
    return _S48;
}

fn sample_colour_0( tile_1 : u32,  node_2 : vec2<f32>) -> f32
{
    var x_4 : f32 = clamp(node_2.x, 0.0f, 34.0f);
    var y_4 : f32 = clamp(node_2.y, 0.0f, 34.0f);
    var xi_0 : i32 = i32(floor(x_4));
    var yi_0 : i32 = i32(floor(y_4));
    var tx_2 : f32 = x_4 - f32(xi_0);
    var ty_2 : f32 = y_4 - f32(yi_0);
    var _S49 : i32 = min(xi_0 + i32(1), i32(34));
    var _S50 : i32 = min(yi_0 + i32(1), i32(34));
    var _S51 : vec3<i32> = vec3<i32>(yi_0, xi_0, i32(0));
    var _S52 : vec3<i32> = vec3<i32>(_S50, xi_0, i32(0));
    var _S53 : vec3<i32> = vec3<i32>(yi_0, _S49, i32(0));
    var _S54 : vec3<i32> = vec3<i32>(_S50, _S49, i32(0));
    var _S55 : f32 = 1.0f - ty_2;
    return ((textureLoad((colours_0[tile_1]), ((_S51)).xy, ((_S51)).z).x) * _S55 + (textureLoad((colours_0[tile_1]), ((_S52)).xy, ((_S52)).z).x) * ty_2) * (1.0f - tx_2) + ((textureLoad((colours_0[tile_1]), ((_S53)).xy, ((_S53)).z).x) * _S55 + (textureLoad((colours_0[tile_1]), ((_S54)).xy, ((_S54)).z).x) * ty_2) * tx_2;
}

fn surface_albedo_0( input_0 : VertexOutput_0) -> vec3<f32>
{
    if((uniforms_0.terrain_0.y) <= 0.0f)
    {
        return uniforms_0.colour_0.xyz * vec3<f32>(input_0.tint_0);
    }
    return vec3<f32>((sample_colour_0(input_0.colour_tile_1, input_0.colour_node_0) * uniforms_0.terrain_0.y * input_0.tint_0));
}

struct pixelOutput_1
{
    @location(0) output_3 : vec4<f32>,
};

struct pixelInput_1
{
    @location(0) normal_4 : vec3<f32>,
    @location(1) world_3 : vec3<f32>,
    @interpolate(flat) @location(2) colour_tile_3 : u32,
    @location(3) colour_node_2 : vec2<f32>,
    @location(4) tint_2 : f32,
};

@fragment
fn fragment_terrain( _S56 : pixelInput_1, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_1
{
    var _S57 : VertexOutput_0 = VertexOutput_0( position_2, _S56.normal_4, _S56.world_3, _S56.colour_tile_3, _S56.colour_node_2, _S56.tint_2 );
    var facet_0 : vec3<f32> = normalize(cross(dpdx(_S56.world_3), dpdy(_S56.world_3)));
    var facet_1 : vec3<f32>;
    if((dot(facet_0, _S56.normal_4)) < 0.0f)
    {
        facet_1 = (vec3<f32>(0) - facet_0);
    }
    else
    {
        facet_1 = facet_0;
    }
    var _S58 : pixelOutput_1 = pixelOutput_1( vec4<f32>(shade_0(facet_1, surface_albedo_0(_S57)), 1.0f) );
    return _S58;
}