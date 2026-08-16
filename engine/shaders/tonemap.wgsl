@binding(0) @group(0) var scene_0 : texture_2d<f32>;

struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_0 : u32) -> VertexOutput_0
{
    var output_0 : VertexOutput_0;
    output_0.position_0 = vec4<f32>(vec2<f32>(f32((((vertex_0 << (u32(1)))) & (u32(2)))), f32((vertex_0 & (u32(2))))) * vec2<f32>(2.0f) - vec2<f32>(1.0f), 0.0f, 1.0f);
    return output_0;
}

fn compress_0( x_0 : f32) -> f32
{
    if(x_0 <= 0.80000001192092896f)
    {
        return x_0;
    }
    return 1.0f - 0.03999999538064003f / (x_0 - 1.60000002384185791f + 1.0f);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

@fragment
fn fragment_main(@builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S1 : vec3<i32> = vec3<i32>(i32(position_1.x), i32(position_1.y), i32(0));
    var c_0 : vec3<f32> = (textureLoad((scene_0), ((_S1)).xy, ((_S1)).z)).xyz;
    var _S2 : pixelOutput_0 = pixelOutput_0( vec4<f32>(compress_0(c_0.x), compress_0(c_0.y), compress_0(c_0.z), 1.0f) );
    return _S2;
}

