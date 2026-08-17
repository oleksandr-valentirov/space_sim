// A polyline in the frame (ROADMAP J1).
//
// Positions in the vertex buffer are already camera-relative, as in
// sphere.slang: the CPU subtracted the camera position in double and narrowed
// the result (ROADMAP F4, engine::camera). Only the projection is left here.
//
// Why not trajectory.slang, which already has a frame transform: that one
// draws relative to Earth-at-time-t and has its own "camera" as a single
// offset, with no basis. It does not care where the player looks -- here that
// matters. The rotating frame (PROJECT.md §7) will arrive here when the frame
// service does, as a replacement for this shader rather than a second render
// path.

// The colour is a vertex attribute rather than a uniform field. There are
// several polylines in a frame, each with its own colour, so in a uniform it
// would need either a dynamic offset or a buffer rewrite between draw calls
// inside one pass -- and all queue writes happen BEFORE the pass, so the last
// one would win. Sixteen bytes per vertex are cheaper than that class of bug.

struct Uniforms
{
    float4x4 projection;
};

ConstantBuffer<Uniforms> uniforms;

struct VertexInput
{
    float3 position : POSITION;
    float4 colour : COLOR;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float4 colour : COLOR;
};

[shader("vertex")]
VertexOutput vertex_main(VertexInput input)
{
    VertexOutput output;
    output.position = mul(uniforms.projection, float4(input.position, 1.0));
    output.colour = input.colour;
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    return input.colour;
}
