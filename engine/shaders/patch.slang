// Патч кубосфери (ROADMAP-PLANETS.md, R1d, R5c).
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
//
// З R1e геометрія патча — **одинична сфера**, спільна для всіх тіл, а розмір
// і поворот конкретного тіла приходять другою матрицею. Інакше два тіла з
// різними радіусами означали б дві копії тих самих вершин.
//
// ## Рельєф: дві точки входу, а не гілка (R5c)
//
// Тіло з тайлами й тіло без них малюються різними пайплайнами. Це не
// оптимізація, а обхід уже спійманої пастки: рантайм-перемикач за uniform-ом
// у ВЕРШИННІЙ стадії на AMD RADV (ACO) мовчки читався неправильно (ROADMAP
// F6), і кадр був порожній без жодної діагностики. Друга причина простіша:
// гладке тіло не має тайла взагалі, і `textureLoad` за невизначеним індексом
// не мусить навіть з'явитися в його програмі.
//
// Нормаль теж різна, і це не косметика. Для гладкої сфери нормаль аналітична
// — напрямок із центра. Для рельєфу такої нормалі не існує: поверхня це
// набір фасеток розміром із клітинку DEM, і єдина чесна нормаль тут —
// геометрична, з похідних світової позиції по екрану. Саме вона й показує
// рельєф тінями на термінаторі, заради чого крок і робиться.

struct Uniforms
{
    float4x4 projection;  // проєкція, помножена на поворот вигляду
    float4x4 model;       // поворот тіла × його радіус, у світових осях
    float4 light_dir;     // напрямок ДО джерела світла, світовий простір
    float4 colour;
    // x — скільки одиниць одиничної сфери в одній одиниці зберігання висоти,
    // тобто `scale_m / radius_m`. Решта — запас, свідомо нульовий.
    float4 terrain;
};

ConstantBuffer<Uniforms> uniforms;

// Початок кожного патча, вже відносно камери й вже повернутий разом із тілом,
// у СВІТОВИХ осях, плюс номер його тайла.
//
// Storage-буфер, а не масив uniform-буферів: D3D12 останніх не дає взагалі
// (PROJECT.md §7, розвідка P0), і per-object дані там живуть саме так.
struct PatchData
{
    float3 origin;
    uint tile;
};

StructuredBuffer<PatchData> patches;

// Висоти тайлів — **bindless-масив текстур**, з першої ж текстури в рушії
// (правило 6 етапу R). Семплера поруч немає, і це не пропуск: тайл лежить
// вузол у вузол із сіткою патча, тож вибірка тут — точний `Load` за цілим
// індексом, а не фільтрація. Масив семплерів з'явиться разом із кольором,
// якому фільтрація справді потрібна.
//
// ⚠ Окрема група прив'язки (set 1), і це вимога wgpu, а не смак: «bind groups
// may not contain both a binding array and a dynamically offset buffer». А
// динамічний зсув у групі 0 — це вибір проходу глибини (R4a), тобто те, що
// нікуди не подінеться. Отже масив живе окремо.
[[vk::binding(0, 1)]]
Texture2D<int> tiles[];

struct VertexInput
{
    float3 offset : POSITION;
    float3 normal : NORMAL;
    uint patch : PATCH;
    // Вузол сітки, спакований як `a | (b << 16)`. Потрібен лише рельєфу:
    // це адреса в тайлі, і взяти її з `SV_VertexID` не можна, бо
    // `base_vertex` зсуває нумерацію на слот кеша.
    uint node : NODE;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
    // Позиція в світових осях відносно камери — потрібна рельєфу, щоб
    // фрагмент узяв із неї геометричну нормаль.
    float3 world : WORLD;
};

// Спільна частина: зсув вершини всередині патча, вже повернутий і масштабований.
float3 place(VertexInput input, float3 offset)
{
    float3 origin = patches[input.patch].origin;
    return origin + mul(uniforms.model, float4(offset, 0.0)).xyz;
}

[shader("vertex")]
VertexOutput vertex_smooth(VertexInput input)
{
    VertexOutput output;
    float3 world = place(input, input.offset);
    output.position = mul(uniforms.projection, float4(world, 1.0));
    // Той самий поворот, що й для зсуву: нормаль, повернута інакше, ніж
    // геометрія, дає освітлення чужого тіла. Радіус у матриці нормаль лише
    // подовжує — `normalize` у фрагменті його прибирає.
    output.normal = mul(uniforms.model, float4(input.normal, 0.0)).xyz;
    output.world = world;
    return output;
}

[shader("vertex")]
VertexOutput vertex_terrain(VertexInput input)
{
    int2 node = int2(int(input.node & 0xffffu), int(input.node >> 16));
    int height = tiles[patches[input.patch].tile].Load(int3(node, 0)).x;

    // Висота зсуває вершину вздовж нормалі патча — на ОДИНИЧНІЙ сфері, бо
    // радіус і поворот застосовує та сама матриця, що й до самого зсуву.
    // Множник `terrain.x` і є `scale_m / radius_m`.
    float3 offset = input.offset + input.normal * (float(height) * uniforms.terrain.x);

    VertexOutput output;
    float3 world = place(input, offset);
    output.position = mul(uniforms.projection, float4(world, 1.0));
    output.normal = mul(uniforms.model, float4(input.normal, 0.0)).xyz;
    output.world = world;
    return output;
}

float3 shade(float3 normal)
{
    float3 n = normalize(normal);
    float diffuse = max(dot(n, uniforms.light_dir.xyz), 0.0);
    return uniforms.colour.rgb * (0.05 + 0.95 * diffuse);
}

[shader("fragment")]
float4 fragment_smooth(VertexOutput input) : SV_Target
{
    return float4(shade(input.normal), 1.0);
}

[shader("fragment")]
float4 fragment_terrain(VertexOutput input) : SV_Target
{
    // Геометрична нормаль фасетки: рельєф це набір площинок розміром із
    // клітинку DEM, і аналітична нормаль сфери про них не знає нічого.
    float3 facet = normalize(cross(ddx(input.world), ddy(input.world)));
    // Похідні дають нормаль з довільним знаком — розвертаємо її назовні за
    // нормаллю сфери, яка завжди правильна за напрямком.
    if (dot(facet, input.normal) < 0.0)
    {
        facet = -facet;
    }
    return float4(shade(facet), 1.0);
}
