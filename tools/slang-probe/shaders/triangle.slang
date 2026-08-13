// Тривіальний шейдер для розвідки P1 (ROADMAP, етап E).
//
// Навмисно мінімальний, але не порожній: тут є те, заради чого Slang і
// беруть — структура з семантиками, яка їде з вершинного у фрагментний,
// і два входи в одному файлі. Якщо це доїде до wgpu, доїде й решта.

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
