// Сфера в реальному масштабі (ROADMAP F5).
//
// Позиції у вершинному буфері вже camera-relative — та сама угода, що в
// depth_quad.slang: камера в (0,0,0), CPU відняв світові координати в
// double й звузив результат (ROADMAP F4, engine::camera). Тут лишається
// тільки проєкція.
//
// Освітлення — тимчасове ламберт-затінення з невеликим ambient, щоб форма
// сфери була видна оком. Це НЕ фінальна модель з PROJECT.md §7 (нуль
// ambient, SH-проба від кольору планети) — та чекає на реальне сонце й
// атмосферу.

struct Uniforms
{
    float4x4 projection;
    float4 light_dir;   // напрямок ДО джерела світла, світовий простір, xyz
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
