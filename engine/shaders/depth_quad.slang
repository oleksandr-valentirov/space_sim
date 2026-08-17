// A flat quad at a given distance, for measuring depth resolution
// (ROADMAP F3).
//
// Vertices are computed from the index and no geometry comes from outside:
// what is checked is the depth buffer, not work with vertex buffers. The quad
// is deliberately larger than the screen, so that the overlap of two of them
// covers the whole frame and flicker cannot be missed at an edge.
//
// Coordinates are already in camera space: the camera sits at (0,0,0) looking
// along -z. This is the camera-relative convention PROJECT.md §7 requires --
// world coordinates never reach a float, not even here.

struct Params
{
    float4x4 projection;
    float4 colour;
    float4 placement;   // xyz -- centre in camera space, w -- half width
};

ConstantBuffer<Params> params;

struct VertexOutput
{
    float4 position : SV_Position;
    float3 colour : COLOR;
};

[shader("vertex")]
VertexOutput vertex_main(uint index: SV_VertexID)
{
    // Two triangles: 0,1,2 and 2,1,3.
    uint corner = (index < 3) ? index : (index == 3 ? 2 : (index == 4 ? 1 : 3));

    float x = ((corner & 1) == 0) ? -1.0 : 1.0;
    float y = ((corner & 2) == 0) ? -1.0 : 1.0;

    float half_size = params.placement.w;
    float3 view = params.placement.xyz + float3(x * half_size, y * half_size, 0.0);

    VertexOutput output;
    output.position = mul(params.projection, float4(view, 1.0));
    output.colour = params.colour.rgb;
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    return float4(input.colour, 1.0);
}
