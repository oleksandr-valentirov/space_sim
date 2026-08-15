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
    @align(4) _pad_0 : f32,
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
};

@binding(0) @group(0) var<uniform> uniforms_0 : Uniforms_std140_0;
@binding(0) @group(1) var tiles_0 : binding_array<texture_2d<i32>>;

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
};

fn place_0( slot_2 : u32,  vertex_1 : ptr<function, PatchVertex_std430_0>,  offset_1 : vec3<f32>) -> VertexOutput_0
{
    var world_1 : vec3<f32> = patches_0[slot_2].origin_0 + (((vec4<f32>(offset_1, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    var output_0 : VertexOutput_0;
    output_0.position_0 = (((vec4<f32>(world_1, 1.0f)) * (mat4x4<f32>(uniforms_0.projection_0.data_0[i32(0)][i32(0)], uniforms_0.projection_0.data_0[i32(1)][i32(0)], uniforms_0.projection_0.data_0[i32(2)][i32(0)], uniforms_0.projection_0.data_0[i32(3)][i32(0)], uniforms_0.projection_0.data_0[i32(0)][i32(1)], uniforms_0.projection_0.data_0[i32(1)][i32(1)], uniforms_0.projection_0.data_0[i32(2)][i32(1)], uniforms_0.projection_0.data_0[i32(3)][i32(1)], uniforms_0.projection_0.data_0[i32(0)][i32(2)], uniforms_0.projection_0.data_0[i32(1)][i32(2)], uniforms_0.projection_0.data_0[i32(2)][i32(2)], uniforms_0.projection_0.data_0[i32(3)][i32(2)], uniforms_0.projection_0.data_0[i32(0)][i32(3)], uniforms_0.projection_0.data_0[i32(1)][i32(3)], uniforms_0.projection_0.data_0[i32(2)][i32(3)], uniforms_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_0.normal_1 = (((vec4<f32>((*vertex_1).normal_0, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    output_0.world_0 = world_1;
    return output_0;
}

@vertex
fn vertex_smooth(@builtin(vertex_index) vertex_2 : u32, @builtin(instance_index) instance_0 : u32) -> VertexOutput_0
{
    var draw_0 : PatchDraw_std430_0 = draws_0[instance_0];
    var _S3 : PatchVertex_std430_0 = vertices_0[node_of_0(vertex_2, draw_0.slot_0, draw_0.mask_0).index_0];
    var _S4 : VertexOutput_0 = place_0(draw_0.slot_0, &(_S3), _S3.offset_0);
    return _S4;
}

fn sample_height_0( patch_0 : ptr<function, PatchData_std430_0>,  grid_1 : vec2<u32>) -> f32
{
    var x_0 : f32 = (*patch_0).window_origin_0.x + f32(grid_1.x) * (*patch_0).window_step_0;
    var y_0 : f32 = (*patch_0).window_origin_0.y + f32(grid_1.y) * (*patch_0).window_step_0;
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
    return (f32((textureLoad((tiles_0[(*patch_0).tile_0]), ((_S7)).xy, ((_S7)).z).x)) * _S13 + f32((textureLoad((tiles_0[(*patch_0).tile_0]), ((_S9)).xy, ((_S9)).z).x)) * ty_0) * (1.0f - tx_0) + (f32((textureLoad((tiles_0[(*patch_0).tile_0]), ((_S11)).xy, ((_S11)).z).x)) * _S13 + f32((textureLoad((tiles_0[(*patch_0).tile_0]), ((_S12)).xy, ((_S12)).z).x)) * ty_0) * tx_0;
}

@vertex
fn vertex_terrain(@builtin(vertex_index) vertex_3 : u32, @builtin(instance_index) instance_1 : u32) -> VertexOutput_0
{
    var draw_1 : PatchDraw_std430_0 = draws_0[instance_1];
    var node_0 : Node_0 = node_of_0(vertex_3, draw_1.slot_0, draw_1.mask_0);
    var _S14 : PatchVertex_std430_0 = vertices_0[node_0.index_0];
    var _S15 : PatchData_std430_0 = patches_0[draw_1.slot_0];
    var _S16 : f32 = sample_height_0(&(_S15), node_0.grid_0);
    var _S17 : VertexOutput_0 = place_0(draw_1.slot_0, &(_S14), _S14.offset_0 + _S14.normal_0 * vec3<f32>((_S16 * uniforms_0.terrain_0.x)));
    return _S17;
}

fn shade_0( normal_2 : vec3<f32>) -> vec3<f32>
{
    return uniforms_0.colour_0.xyz * vec3<f32>((0.05000000074505806f + 0.94999998807907104f * max(dot(normalize(normal_2), uniforms_0.light_dir_0.xyz), 0.0f)));
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) normal_3 : vec3<f32>,
    @location(1) world_2 : vec3<f32>,
};

@fragment
fn fragment_smooth( _S18 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S19 : pixelOutput_0 = pixelOutput_0( vec4<f32>(shade_0(_S18.normal_3), 1.0f) );
    return _S19;
}

struct pixelOutput_1
{
    @location(0) output_2 : vec4<f32>,
};

struct pixelInput_1
{
    @location(0) normal_4 : vec3<f32>,
    @location(1) world_3 : vec3<f32>,
};

@fragment
fn fragment_terrain( _S20 : pixelInput_1, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_1
{
    var facet_0 : vec3<f32> = normalize(cross(dpdx(_S20.world_3), dpdy(_S20.world_3)));
    var facet_1 : vec3<f32>;
    if((dot(facet_0, _S20.normal_4)) < 0.0f)
    {
        facet_1 = (vec3<f32>(0) - facet_0);
    }
    else
    {
        facet_1 = facet_0;
    }
    var _S21 : pixelOutput_1 = pixelOutput_1( vec4<f32>(shade_0(facet_1), 1.0f) );
    return _S21;
}