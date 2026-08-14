// Ламана в кадрі (ROADMAP J1).
//
// Позиції у вершинному буфері вже camera-relative, як і в sphere.slang: CPU
// відняв позицію камери в double й звузив результат (ROADMAP F4,
// engine::camera). Тут лишається тільки проєкція.
//
// Чому не trajectory.slang, у якого вже є перетворення фрейму: той малює
// відносно Землі-в-момент-t і має власну «камеру» одним зсувом, без базису.
// Йому байдуже, куди дивиться гравець, — а тут не байдуже. Обертовий фрейм
// (PROJECT.md §7) приїде сюди тоді ж, коли з'явиться сервіс фреймів, і
// заміною цього шейдера, а не другим шляхом рендера.

// Колір — атрибут вершини, а не поле uniform. Ламаних у кадрі кілька, і в
// кожної свій колір, тож у uniform він вимагав би або динамічного зсуву, або
// перезапису буфера між викликами draw усередині одного проходу — а записи в
// чергу всі відбуваються ДО проходу, тож виграв би останній. Шістнадцять
// байтів на вершину дешевші за цей клас помилки.

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
