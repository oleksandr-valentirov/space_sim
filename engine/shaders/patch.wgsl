enable wgpu_binding_array;

struct PatchData_std430_0
{
    @align(16) origin_0 : vec3<f32>,
    @align(4) tile_0 : u32,
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

struct VertexInput_0
{
     offset_0 : vec3<f32>,
     normal_0 : vec3<f32>,
     patch_0 : u32,
     node_0 : u32,
};

fn place_0( input_0 : VertexInput_0,  offset_1 : vec3<f32>) -> vec3<f32>
{
    return patches_0[input_0.patch_0].origin_0 + (((vec4<f32>(offset_1, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
}

struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) normal_1 : vec3<f32>,
    @location(1) world_0 : vec3<f32>,
};

struct vertexInput_0
{
    @location(0) offset_2 : vec3<f32>,
    @location(1) normal_2 : vec3<f32>,
    @location(2) patch_1 : u32,
    @location(3) node_1 : u32,
};

@vertex
fn vertex_smooth( _S1 : vertexInput_0) -> VertexOutput_0
{
    var _S2 : VertexInput_0 = VertexInput_0( _S1.offset_2, _S1.normal_2, _S1.patch_1, _S1.node_1 );
    var world_1 : vec3<f32> = place_0(_S2, _S1.offset_2);
    var output_0 : VertexOutput_0;
    output_0.position_0 = (((vec4<f32>(world_1, 1.0f)) * (mat4x4<f32>(uniforms_0.projection_0.data_0[i32(0)][i32(0)], uniforms_0.projection_0.data_0[i32(1)][i32(0)], uniforms_0.projection_0.data_0[i32(2)][i32(0)], uniforms_0.projection_0.data_0[i32(3)][i32(0)], uniforms_0.projection_0.data_0[i32(0)][i32(1)], uniforms_0.projection_0.data_0[i32(1)][i32(1)], uniforms_0.projection_0.data_0[i32(2)][i32(1)], uniforms_0.projection_0.data_0[i32(3)][i32(1)], uniforms_0.projection_0.data_0[i32(0)][i32(2)], uniforms_0.projection_0.data_0[i32(1)][i32(2)], uniforms_0.projection_0.data_0[i32(2)][i32(2)], uniforms_0.projection_0.data_0[i32(3)][i32(2)], uniforms_0.projection_0.data_0[i32(0)][i32(3)], uniforms_0.projection_0.data_0[i32(1)][i32(3)], uniforms_0.projection_0.data_0[i32(2)][i32(3)], uniforms_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_0.normal_1 = (((vec4<f32>(_S1.normal_2, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    output_0.world_0 = world_1;
    return output_0;
}

struct vertexInput_1
{
    @location(0) offset_3 : vec3<f32>,
    @location(1) normal_3 : vec3<f32>,
    @location(2) patch_2 : u32,
    @location(3) node_2 : u32,
};

@vertex
fn vertex_terrain( _S3 : vertexInput_1) -> VertexOutput_0
{
    var _S4 : VertexInput_0 = VertexInput_0( _S3.offset_3, _S3.normal_3, _S3.patch_2, _S3.node_2 );
    var _S5 : vec3<i32> = vec3<i32>(vec2<i32>(i32(((_S3.node_2) & (u32(65535)))), i32(((_S3.node_2) >> (u32(16))))), i32(0));
    var world_2 : vec3<f32> = place_0(_S4, _S3.offset_3 + _S3.normal_3 * vec3<f32>((f32((textureLoad((tiles_0[patches_0[_S3.patch_2].tile_0]), ((_S5)).xy, ((_S5)).z).x)) * uniforms_0.terrain_0.x)));
    var output_1 : VertexOutput_0;
    output_1.position_0 = (((vec4<f32>(world_2, 1.0f)) * (mat4x4<f32>(uniforms_0.projection_0.data_0[i32(0)][i32(0)], uniforms_0.projection_0.data_0[i32(1)][i32(0)], uniforms_0.projection_0.data_0[i32(2)][i32(0)], uniforms_0.projection_0.data_0[i32(3)][i32(0)], uniforms_0.projection_0.data_0[i32(0)][i32(1)], uniforms_0.projection_0.data_0[i32(1)][i32(1)], uniforms_0.projection_0.data_0[i32(2)][i32(1)], uniforms_0.projection_0.data_0[i32(3)][i32(1)], uniforms_0.projection_0.data_0[i32(0)][i32(2)], uniforms_0.projection_0.data_0[i32(1)][i32(2)], uniforms_0.projection_0.data_0[i32(2)][i32(2)], uniforms_0.projection_0.data_0[i32(3)][i32(2)], uniforms_0.projection_0.data_0[i32(0)][i32(3)], uniforms_0.projection_0.data_0[i32(1)][i32(3)], uniforms_0.projection_0.data_0[i32(2)][i32(3)], uniforms_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_1.normal_1 = (((vec4<f32>(_S3.normal_3, 0.0f)) * (mat4x4<f32>(uniforms_0.model_0.data_0[i32(0)][i32(0)], uniforms_0.model_0.data_0[i32(1)][i32(0)], uniforms_0.model_0.data_0[i32(2)][i32(0)], uniforms_0.model_0.data_0[i32(3)][i32(0)], uniforms_0.model_0.data_0[i32(0)][i32(1)], uniforms_0.model_0.data_0[i32(1)][i32(1)], uniforms_0.model_0.data_0[i32(2)][i32(1)], uniforms_0.model_0.data_0[i32(3)][i32(1)], uniforms_0.model_0.data_0[i32(0)][i32(2)], uniforms_0.model_0.data_0[i32(1)][i32(2)], uniforms_0.model_0.data_0[i32(2)][i32(2)], uniforms_0.model_0.data_0[i32(3)][i32(2)], uniforms_0.model_0.data_0[i32(0)][i32(3)], uniforms_0.model_0.data_0[i32(1)][i32(3)], uniforms_0.model_0.data_0[i32(2)][i32(3)], uniforms_0.model_0.data_0[i32(3)][i32(3)])))).xyz;
    output_1.world_0 = world_2;
    return output_1;
}

fn shade_0( normal_4 : vec3<f32>) -> vec3<f32>
{
    return uniforms_0.colour_0.xyz * vec3<f32>((0.05000000074505806f + 0.94999998807907104f * max(dot(normalize(normal_4), uniforms_0.light_dir_0.xyz), 0.0f)));
}

struct pixelOutput_0
{
    @location(0) output_2 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) normal_5 : vec3<f32>,
    @location(1) world_3 : vec3<f32>,
};

@fragment
fn fragment_smooth( _S6 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S7 : pixelOutput_0 = pixelOutput_0( vec4<f32>(shade_0(_S6.normal_5), 1.0f) );
    return _S7;
}

struct pixelOutput_1
{
    @location(0) output_3 : vec4<f32>,
};

struct pixelInput_1
{
    @location(0) normal_6 : vec3<f32>,
    @location(1) world_4 : vec3<f32>,
};

@fragment
fn fragment_terrain( _S8 : pixelInput_1, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_1
{
    var facet_0 : vec3<f32> = normalize(cross(dpdx(_S8.world_4), dpdy(_S8.world_4)));
    var facet_1 : vec3<f32>;
    if((dot(facet_0, _S8.normal_6)) < 0.0f)
    {
        facet_1 = (vec3<f32>(0) - facet_0);
    }
    else
    {
        facet_1 = facet_0;
    }
    var _S9 : pixelOutput_1 = pixelOutput_1( vec4<f32>(shade_0(facet_1), 1.0f) );
    return _S9;
}