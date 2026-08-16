// The same triangle, but the vertices arrive in a buffer rather than being
// computed from SV_VertexID. Exists for one question of P1 reconnaissance.
//
// triangle.slang does not load as SPIR-V: naga rejects the DrawParameters
// capability that Slang declares for SV_VertexID. The question this file
// answers is whether SPIR-V breaks in general or only for that construct. The
// answer changes the conclusion: "the route is unusable" and "the route is
// usable except for one very common idiom" are different things.
//
// The probe does not try to draw with it: that would need vertex buffers and a
// layout, and the question here is not about drawing but about what naga
// accepts.

struct VertexOutput
{
    float4 position : SV_Position;
    float3 color : COLOR;
};

[shader("vertex")]
VertexOutput vertex_main(float2 position: POSITION, float3 color: COLOR)
{
    VertexOutput output;
    output.position = float4(position, 0.0, 1.0);
    output.color = color;
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    return float4(input.color, 1.0);
}
