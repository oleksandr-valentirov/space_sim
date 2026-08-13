struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct Uniforms_std140_0
{
    @align(16) projection_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) mu_0 : f32,
    @align(4) _pad0_0 : f32,
    @align(8) _pad1_0 : vec2<f32>,
    @align(16) view_offset_0 : vec3<f32>,
    @align(4) _pad2_0 : f32,
    @align(16) colour_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> uniforms_0 : Uniforms_std140_0;
fn synodic_basis_0( moon_local_0 : vec3<f32>,  z_axis_0 : vec3<f32>,  length_scale_0 : ptr<function, f32>) -> mat3x3<f32>
{
    var _S1 : f32 = length(moon_local_0);
    (*length_scale_0) = _S1;
    var x_axis_0 : vec3<f32> = moon_local_0 / vec3<f32>(_S1);
    return mat3x3<f32>(x_axis_0, cross(z_axis_0, x_axis_0), z_axis_0);
}

fn project_0( view_basis_pos_0 : vec3<f32>,  u_0 : ptr<function, Uniforms_std140_0>) -> vec4<f32>
{
    return (((vec4<f32>(vec3<f32>(view_basis_pos_0.x, view_basis_pos_0.z, - view_basis_pos_0.y) + (*u_0).view_offset_0, 1.0f)) * (mat4x4<f32>((*u_0).projection_0.data_0[i32(0)][i32(0)], (*u_0).projection_0.data_0[i32(1)][i32(0)], (*u_0).projection_0.data_0[i32(2)][i32(0)], (*u_0).projection_0.data_0[i32(3)][i32(0)], (*u_0).projection_0.data_0[i32(0)][i32(1)], (*u_0).projection_0.data_0[i32(1)][i32(1)], (*u_0).projection_0.data_0[i32(2)][i32(1)], (*u_0).projection_0.data_0[i32(3)][i32(1)], (*u_0).projection_0.data_0[i32(0)][i32(2)], (*u_0).projection_0.data_0[i32(1)][i32(2)], (*u_0).projection_0.data_0[i32(2)][i32(2)], (*u_0).projection_0.data_0[i32(3)][i32(2)], (*u_0).projection_0.data_0[i32(0)][i32(3)], (*u_0).projection_0.data_0[i32(1)][i32(3)], (*u_0).projection_0.data_0[i32(2)][i32(3)], (*u_0).projection_0.data_0[i32(3)][i32(3)]))));
}

struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
};

struct vertexInput_0
{
    @location(0) vessel_local_0 : vec3<f32>,
    @location(1) moon_local_1 : vec3<f32>,
    @location(2) z_axis_1 : vec3<f32>,
};

@vertex
fn vertex_geocentric( _S2 : vertexInput_0) -> VertexOutput_0
{
    var length_scale_1 : f32;
    var geocentric_0 : vec3<f32> = (((_S2.vessel_local_0) * (synodic_basis_0(_S2.moon_local_1, _S2.z_axis_1, &(length_scale_1)))));
    var output_0 : VertexOutput_0;
    var _S3 : Uniforms_std140_0 = uniforms_0;
    var _S4 : vec4<f32> = project_0(geocentric_0, &(_S3));
    output_0.position_0 = _S4;
    return output_0;
}

struct vertexInput_1
{
    @location(0) vessel_local_1 : vec3<f32>,
    @location(1) moon_local_2 : vec3<f32>,
    @location(2) z_axis_2 : vec3<f32>,
};

@vertex
fn vertex_rotating( _S5 : vertexInput_1) -> VertexOutput_0
{
    var length_scale_2 : f32;
    var basis_0 : mat3x3<f32> = synodic_basis_0(_S5.moon_local_2, _S5.z_axis_2, &(length_scale_2));
    var rotating_0 : vec3<f32> = (((_S5.vessel_local_1 - vec3<f32>(uniforms_0.mu_0) * _S5.moon_local_2) * (basis_0))) / vec3<f32>(length_scale_2);
    var output_1 : VertexOutput_0;
    var _S6 : Uniforms_std140_0 = uniforms_0;
    var _S7 : vec4<f32> = project_0(rotating_0, &(_S6));
    output_1.position_0 = _S7;
    return output_1;
}

struct pixelOutput_0
{
    @location(0) output_2 : vec4<f32>,
};

@fragment
fn fragment_main() -> pixelOutput_0
{
    var _S8 : pixelOutput_0 = pixelOutput_0( uniforms_0.colour_0 );
    return _S8;
}

