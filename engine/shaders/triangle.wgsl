var<private> COLOURS_0 : array<vec3<f32>, i32(3)> = array<vec3<f32>, i32(3)>( vec3<f32>(1.0f, 0.0f, 0.0f), vec3<f32>(0.0f, 1.0f, 0.0f), vec3<f32>(0.0f, 0.0f, 1.0f) );
var<private> POSITIONS_0 : array<vec2<f32>, i32(3)> = array<vec2<f32>, i32(3)>( vec2<f32>(0.0f, 0.60000002384185791f), vec2<f32>(-0.60000002384185791f, -0.60000002384185791f), vec2<f32>(0.60000002384185791f, -0.60000002384185791f) );
struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) color_0 : vec3<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) index_0 : u32) -> VertexOutput_0
{
    var output_0 : VertexOutput_0;
    output_0.position_0 = vec4<f32>(POSITIONS_0[index_0], 0.0f, 1.0f);
    output_0.color_0 = COLOURS_0[index_0];
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) color_1 : vec3<f32>,
};

@fragment
fn fragment_main( _S1 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S2 : pixelOutput_0 = pixelOutput_0( vec4<f32>(_S1.color_1, 1.0f) );
    return _S2;
}

