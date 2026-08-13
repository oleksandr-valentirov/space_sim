// Перший трикутник (ROADMAP F2).
//
// Вершини рахуються з індексу, буферів немає: на цьому кроці перевіряється
// шлях «шейдер доїхав і намалював», а не робота з геометрією. Буфери
// прийдуть із F5, коли з'явиться справжня сфера.
//
// Компілюється в WGSL: sh scripts/build_shaders.sh
// Чому WGSL, а не SPIR-V — ROADMAP P1.

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
