struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct Uniforms_std140_0
{
    @align(16) projection_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) light_dir_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> uniforms_0 : Uniforms_std140_0;
struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) normal_0 : vec3<f32>,
    @location(1) colour_0 : vec4<f32>,
    @interpolate(flat) @location(2) material_0 : vec2<f32>,
    @interpolate(flat) @location(3) shine_dir_0 : vec3<f32>,
    @interpolate(flat) @location(4) shine_rgb_0 : vec3<f32>,
    @location(5) view_0 : vec3<f32>,
};

struct vertexInput_0
{
    @location(0) position_1 : vec3<f32>,
    @location(1) normal_1 : vec3<f32>,
    @location(2) colour_1 : vec4<f32>,
    @location(3) material_1 : vec2<f32>,
    @location(4) shine_dir_1 : vec3<f32>,
    @location(5) shine_rgb_1 : vec3<f32>,
};

@vertex
fn vertex_main( _S1 : vertexInput_0) -> VertexOutput_0
{
    var output_0 : VertexOutput_0;
    output_0.position_0 = (((vec4<f32>(_S1.position_1, 1.0f)) * (mat4x4<f32>(uniforms_0.projection_0.data_0[i32(0)][i32(0)], uniforms_0.projection_0.data_0[i32(1)][i32(0)], uniforms_0.projection_0.data_0[i32(2)][i32(0)], uniforms_0.projection_0.data_0[i32(3)][i32(0)], uniforms_0.projection_0.data_0[i32(0)][i32(1)], uniforms_0.projection_0.data_0[i32(1)][i32(1)], uniforms_0.projection_0.data_0[i32(2)][i32(1)], uniforms_0.projection_0.data_0[i32(3)][i32(1)], uniforms_0.projection_0.data_0[i32(0)][i32(2)], uniforms_0.projection_0.data_0[i32(1)][i32(2)], uniforms_0.projection_0.data_0[i32(2)][i32(2)], uniforms_0.projection_0.data_0[i32(3)][i32(2)], uniforms_0.projection_0.data_0[i32(0)][i32(3)], uniforms_0.projection_0.data_0[i32(1)][i32(3)], uniforms_0.projection_0.data_0[i32(2)][i32(3)], uniforms_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_0.normal_0 = _S1.normal_1;
    output_0.colour_0 = _S1.colour_1;
    output_0.material_0 = _S1.material_1;
    output_0.shine_dir_0 = _S1.shine_dir_1;
    output_0.shine_rgb_0 = _S1.shine_rgb_1;
    output_0.view_0 = (vec3<f32>(0) - _S1.position_1);
    return output_0;
}

fn fresnel_0( f0_0 : vec3<f32>,  v_dot_h_0 : f32) -> vec3<f32>
{
    var t_0 : f32 = saturate(1.0f - v_dot_h_0);
    return f0_0 + (vec3<f32>(1.0f) - f0_0) * vec3<f32>((t_0 * t_0 * t_0 * t_0 * t_0));
}

fn distribution_0( n_dot_h_0 : f32,  roughness_0 : f32) -> f32
{
    var _S2 : f32 = max(roughness_0, 0.04500000178813934f);
    var a_0 : f32 = _S2 * _S2;
    var a2_0 : f32 = a_0 * a_0;
    var d_0 : f32 = n_dot_h_0 * n_dot_h_0 * (a2_0 - 1.0f) + 1.0f;
    return a2_0 / (3.14159274101257324f * d_0 * d_0);
}

fn visibility_0( n_dot_v_0 : f32,  n_dot_l_0 : f32,  roughness_1 : f32) -> f32
{
    var _S3 : f32 = max(roughness_1, 0.04500000178813934f);
    var a_1 : f32 = _S3 * _S3;
    var a2_1 : f32 = a_1 * a_1;
    var _S4 : f32 = 1.0f - a2_1;
    return 0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S4 + a2_1) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S4 + a2_1), 1.00000000317107685e-30f);
}

fn radiance_0( n_0 : vec3<f32>,  v_0 : vec3<f32>,  l_0 : vec3<f32>,  base_0 : vec3<f32>,  roughness_2 : f32,  metallic_0 : f32) -> vec3<f32>
{
    var n_dot_l_1 : f32 = dot(n_0, l_0);
    var n_dot_v_1 : f32 = dot(n_0, v_0);
    var _S5 : bool;
    if(n_dot_l_1 <= 0.0f)
    {
        _S5 = true;
    }
    else
    {
        _S5 = n_dot_v_1 <= 0.0f;
    }
    if(_S5)
    {
        return vec3<f32>(0.0f);
    }
    var h_0 : vec3<f32> = normalize(v_0 + l_0);
    var f_0 : vec3<f32> = fresnel_0(mix(vec3<f32>(0.03999999910593033f), base_0, vec3<f32>(metallic_0)), saturate(dot(v_0, h_0)));
    return ((vec3<f32>(1.0f) - f_0) * vec3<f32>((1.0f - metallic_0)) * base_0 / vec3<f32>(3.14159274101257324f) + vec3<f32>((distribution_0(saturate(dot(n_0, h_0)), roughness_2) * visibility_0(n_dot_v_1, n_dot_l_1, roughness_2))) * f_0) * vec3<f32>(n_dot_l_1);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) normal_2 : vec3<f32>,
    @location(1) colour_2 : vec4<f32>,
    @interpolate(flat) @location(2) material_2 : vec2<f32>,
    @interpolate(flat) @location(3) shine_dir_2 : vec3<f32>,
    @interpolate(flat) @location(4) shine_rgb_2 : vec3<f32>,
    @location(5) view_1 : vec3<f32>,
};

@fragment
fn fragment_main( _S6 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var n_1 : vec3<f32> = normalize(_S6.normal_2);
    var v_1 : vec3<f32> = normalize(_S6.view_1);
    var n_2 : vec3<f32>;
    if((dot(n_1, v_1)) < 0.0f)
    {
        n_2 = (vec3<f32>(0) - n_1);
    }
    else
    {
        n_2 = n_1;
    }
    var roughness_3 : f32 = _S6.material_2.x;
    var metallic_1 : f32 = _S6.material_2.y;
    var base_1 : vec3<f32> = _S6.colour_2.xyz;
    var _S7 : pixelOutput_0 = pixelOutput_0( vec4<f32>(radiance_0(n_2, v_1, uniforms_0.light_dir_0.xyz, base_1, roughness_3, metallic_1) + radiance_0(n_2, v_1, normalize(_S6.shine_dir_2), base_1, roughness_3, metallic_1) * _S6.shine_rgb_2, 1.0f) );
    return _S7;
}

