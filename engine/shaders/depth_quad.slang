// Плаский чотирикутник на заданій відстані — для заміру роздільності
// глибини (ROADMAP F3).
//
// Вершини рахуються з індексу, геометрії ззовні немає: перевіряється буфер
// глибини, а не робота з буферами вершин. Чотирикутник свідомо більший за
// екран, щоб перекриття двох таких було на весь кадр і мерехтіння не можна
// було проґавити на краю.
//
// Координати одразу в камерному просторі: камера в (0,0,0), дивиться вздовж
// -z. Це вже той camera-relative, який PROJECT.md §7 вимагає, — світові
// координати у float не потрапляють навіть тут.

struct Params
{
    float4x4 projection;
    float4 colour;
    float4 placement;   // x — z у камерному просторі (від'ємне), y — півширина
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
    // Два трикутники: 0,1,2 і 2,1,3.
    uint corner = (index < 3) ? index : (index == 3 ? 2 : (index == 4 ? 1 : 3));

    float x = ((corner & 1) == 0) ? -1.0 : 1.0;
    float y = ((corner & 2) == 0) ? -1.0 : 1.0;

    float half_size = params.placement.y;
    float3 view = float3(x * half_size, y * half_size, params.placement.x);

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
