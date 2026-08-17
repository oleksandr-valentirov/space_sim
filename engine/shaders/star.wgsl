struct Star_std430_0
{
    @align(16) dir_flux_0 : vec4<f32>,
};

@binding(0) @group(0) var<storage, read> stars_0 : array<Star_std430_0>;

struct StarView_std140_0
{
    @align(16) right_0 : vec4<f32>,
    @align(16) up_0 : vec4<f32>,
    @align(16) forward_0 : vec4<f32>,
    @align(16) params_0 : vec4<f32>,
};

@binding(1) @group(0) var<uniform> view_0 : StarView_std140_0;
struct StarVertex_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) offset_0 : vec2<f32>,
    @interpolate(flat) @location(1) flux_0 : f32,
};

@vertex
fn vertex_star(@builtin(vertex_index) id_0 : u32) -> StarVertex_0
{
    var output_0 : StarVertex_0;
    var star_0 : Star_std430_0 = stars_0[id_0 / u32(6)];
    var dir_0 : vec3<f32> = star_0.dir_flux_0.xyz;
    var corner_0 : u32 = id_0 % u32(6);
    var index_0 : u32;
    if(corner_0 < u32(3))
    {
        index_0 = corner_0;
    }
    else
    {
        if(corner_0 == u32(3))
        {
            index_0 = u32(2);
        }
        else
        {
            if(corner_0 == u32(4))
            {
                index_0 = u32(1);
            }
            else
            {
                index_0 = u32(3);
            }
        }
    }
    var offset_1 : vec2<f32> = vec2<f32>(f32((index_0 & (u32(1)))) * 2.0f - 1.0f, f32((index_0 >> (u32(1)))) * 2.0f - 1.0f);
    var ahead_0 : f32 = dot(dir_0, view_0.forward_0.xyz);
    var ndc_0 : vec2<f32> = vec2<f32>(dot(dir_0, view_0.right_0.xyz) / max(view_0.right_0.w, 9.99999997475242708e-07f), dot(dir_0, view_0.up_0.xyz) / max(view_0.up_0.w, 9.99999997475242708e-07f));
    if(ahead_0 <= 9.99999997475242708e-07f)
    {
        output_0.position_0 = vec4<f32>(10000.0f, 10000.0f, 0.0f, 1.0f);
        output_0.offset_0 = vec2<f32>(0.0f, 0.0f);
        output_0.flux_0 = 0.0f;
        return output_0;
    }
    output_0.position_0 = vec4<f32>(ndc_0 / vec2<f32>(ahead_0) + offset_1 * vec2<f32>(view_0.params_0.x / view_0.params_0.y, view_0.params_0.x / view_0.params_0.z), 0.0f, 1.0f);
    output_0.offset_0 = offset_1;
    output_0.flux_0 = star_0.dir_flux_0.w;
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) offset_2 : vec2<f32>,
    @interpolate(flat) @location(1) flux_1 : f32,
};

@fragment
fn fragment_star( _S1 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S2 : vec2<f32> = _S1.offset_2;
    var _S3 : f32 = max(0.0f, 1.0f - dot(_S2, _S2));
    var radiance_0 : f32 = view_0.params_0.w * _S1.flux_1 * (_S3 * _S3);
    var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>(radiance_0, radiance_0, radiance_0, 1.0f) );
    return _S4;
}

