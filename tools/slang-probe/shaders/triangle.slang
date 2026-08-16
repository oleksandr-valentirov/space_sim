// A trivial shader for P1 reconnaissance (ROADMAP, stage E).
//
// Deliberately minimal but not empty: it holds what Slang is taken for -- a
// struct with semantics travelling from vertex to fragment, and two entry
// points in one file. If this reaches wgpu, the rest will too.

struct VertexOutput
{
    float4 position : SV_Position;
    float3 color : COLOR;
};

static const float2 POSITIONS[3] = {
    float2(0.0, 0.6),
    float2(-0.6, -0.6),
    float2(0.6, -0.6),
};

static const float3 COLOURS[3] = {
    float3(1.0, 0.0, 0.0),
    float3(0.0, 1.0, 0.0),
    float3(0.0, 0.0, 1.0),
};

[shader("vertex")]
VertexOutput vertex_main(uint index: SV_VertexID)
{
    VertexOutput output;
    output.position = float4(POSITIONS[index], 0.0, 1.0);
    output.color = COLOURS[index];
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    return float4(input.color, 1.0);
}
