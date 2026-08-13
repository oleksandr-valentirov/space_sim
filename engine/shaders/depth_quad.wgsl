struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct Params_std140_0
{
    @align(16) projection_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) colour_0 : vec4<f32>,
    @align(16) placement_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> params_0 : Params_std140_0;
struct VertexOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) colour_1 : vec3<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) index_0 : u32) -> VertexOutput_0
{
    var corner_0 : u32;
    if(index_0 < u32(3))
    {
        corner_0 = index_0;
    }
    else
    {
        var _S1 : i32;
        if(index_0 == u32(3))
        {
            _S1 = i32(2);
        }
        else
        {
            if(index_0 == u32(4))
            {
                _S1 = i32(1);
            }
            else
            {
                _S1 = i32(3);
            }
        }
        corner_0 = u32(_S1);
    }
    var x_0 : f32;
    if(((corner_0 & (u32(1)))) == u32(0))
    {
        x_0 = -1.0f;
    }
    else
    {
        x_0 = 1.0f;
    }
    var y_0 : f32;
    if(((corner_0 & (u32(2)))) == u32(0))
    {
        y_0 = -1.0f;
    }
    else
    {
        y_0 = 1.0f;
    }
    var half_size_0 : f32 = params_0.placement_0.w;
    var output_0 : VertexOutput_0;
    output_0.position_0 = (((vec4<f32>(params_0.placement_0.xyz + vec3<f32>(x_0 * half_size_0, y_0 * half_size_0, 0.0f), 1.0f)) * (mat4x4<f32>(params_0.projection_0.data_0[i32(0)][i32(0)], params_0.projection_0.data_0[i32(1)][i32(0)], params_0.projection_0.data_0[i32(2)][i32(0)], params_0.projection_0.data_0[i32(3)][i32(0)], params_0.projection_0.data_0[i32(0)][i32(1)], params_0.projection_0.data_0[i32(1)][i32(1)], params_0.projection_0.data_0[i32(2)][i32(1)], params_0.projection_0.data_0[i32(3)][i32(1)], params_0.projection_0.data_0[i32(0)][i32(2)], params_0.projection_0.data_0[i32(1)][i32(2)], params_0.projection_0.data_0[i32(2)][i32(2)], params_0.projection_0.data_0[i32(3)][i32(2)], params_0.projection_0.data_0[i32(0)][i32(3)], params_0.projection_0.data_0[i32(1)][i32(3)], params_0.projection_0.data_0[i32(2)][i32(3)], params_0.projection_0.data_0[i32(3)][i32(3)]))));
    output_0.colour_1 = params_0.colour_0.xyz;
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) colour_2 : vec3<f32>,
};

@fragment
fn fragment_main( _S2 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S3 : pixelOutput_0 = pixelOutput_0( vec4<f32>(_S2.colour_2, 1.0f) );
    return _S3;
}

