// Корабель у кадрі (етап V, крок V2).
//
// Позиції у вершинному буфері вже camera-relative — та сама угода, що в
// line.slang і sphere.slang: CPU відняв позицію камери в double й звузив
// результат (ROADMAP F4, engine::camera). Тут лишається тільки проєкція.
//
// Нормалі приходять **уже повернутими в світ**: корабель має власну
// орієнтацію, і повертає її той самий CPU-прохід, що й позиції. Робити це в
// шейдері означало б другу матрицю на корабель, тобто ще один uniform із
// динамічним зсувом — при тому, що вершин тут півтори тисячі, а не мільйон.
//
// Освітлення — тимчасовий ламберт із невеликим ambient, рівно як у sphere.slang,
// і з тією самою заувагою: це НЕ модель з PROJECT.md §7 (нуль ambient, сяйво
// планети SH-пробою). Вона чекає на матеріали, тобто на етап T, і на напрямок
// на світило з ефемериди — борг D16.

// Колір сюди не входить: він атрибут вершини, з тієї самої причини, що в
// line.slang. Кораблів у кадрі кілька, виклик малювання на кожен, а всі записи
// в чергу відбуваються ДО проходу — тобто з uniform виграв би останній.
struct Uniforms
{
    float4x4 projection;
    float4 light_dir;   // напрямок ДО світила, світовий простір, xyz
};

// Динамічний зсув вибирає прохід глибини — так само, як у ламаних. Буфер
// один, крок 256 байтів (frame::PASS_STRIDE).
ConstantBuffer<Uniforms> uniforms;

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float4 colour : COLOR;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
    float4 colour : COLOR;
};

[shader("vertex")]
VertexOutput vertex_main(VertexInput input)
{
    VertexOutput output;
    output.position = mul(uniforms.projection, float4(input.position, 1.0));
    output.normal = input.normal;
    output.colour = input.colour;
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    float3 n = normalize(input.normal);
    float diffuse = max(dot(n, uniforms.light_dir.xyz), 0.0);
    float shade = 0.05 + 0.95 * diffuse;
    return float4(input.colour.rgb * shade, 1.0);
}
