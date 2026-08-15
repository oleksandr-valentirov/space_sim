// Патч кубосфери (ROADMAP-PLANETS.md, R1d).
//
// Відмінність від sphere.slang одна, і вона вся тут: вершини приходять
// **зсувами від початку патча**, а не camera-relative позиціями. CPU
// віднімає камеру рівно раз на патч (у double), а не раз на вершину — саме
// заради цього патч і є одиницею (правило 1 етапу R).
//
// Отже шейдер має зробити те, що раніше робив CPU: додати зсув і повернути
// суму в камерний простір. Поворот приїжджає вже вмноженим у projection —
// перенесення в матриці немає, бо його вже виконало віднімання на CPU.
//
// Освітлення те саме, що в sphere.slang, і так само тимчасове.

struct Uniforms
{
    float4x4 projection;  // проєкція, помножена на поворот вигляду
    float4 light_dir;     // напрямок ДО джерела світла, світовий простір
    float4 colour;
};

ConstantBuffer<Uniforms> uniforms;

// Початок кожного патча, вже відносно камери, у СВІТОВИХ осях.
//
// Storage-буфер, а не масив uniform-буферів: D3D12 останніх не дає взагалі
// (PROJECT.md §7, розвідка P0), і per-object дані там живуть саме так.
StructuredBuffer<float4> patch_origins;

struct VertexInput
{
    float3 offset : POSITION;
    float3 normal : NORMAL;
    uint patch : PATCH;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
};

[shader("vertex")]
VertexOutput vertex_main(VertexInput input)
{
    float3 origin = patch_origins[input.patch].xyz;

    VertexOutput output;
    output.position = mul(uniforms.projection, float4(origin + input.offset, 1.0));
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
