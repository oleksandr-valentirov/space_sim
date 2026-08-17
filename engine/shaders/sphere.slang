// A sphere at real scale (ROADMAP F5).
//
// Positions in the vertex buffer are already camera-relative -- the same
// convention as in depth_quad.slang: the camera sits at (0,0,0), the CPU
// subtracted the world coordinates in double and narrowed the result
// (ROADMAP F4, engine::camera). Only the projection is left here.
//
// The lighting is temporary Lambert shading with a little ambient, so that
// the shape of the sphere is visible. This is NOT the final model of
// PROJECT.md §7 (zero ambient, an SH probe from the planet's colour) -- that
// waits for a real sun and an atmosphere.

struct Uniforms
{
    float4x4 projection;
    float4 light_dir;   // direction TO the light source, world space, xyz
    float4 colour;
};

ConstantBuffer<Uniforms> uniforms;

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
};

[shader("vertex")]
VertexOutput vertex_main(VertexInput input)
{
    VertexOutput output;
    output.position = mul(uniforms.projection, float4(input.position, 1.0));
    output.normal = input.normal;
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    float3 n = normalize(input.normal);
    float diffuse = max(dot(n, uniforms.light_dir.xyz), 0.0);
    float shade = 0.05 + 0.95 * diffuse;
    return float4(uniforms.colour.rgb * shade, 1.0);
}
