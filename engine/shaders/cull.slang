// Відбір патчів у compute (ROADMAP-PLANETS.md, R6b).
//
// Те саме, що робить `engine::cull` на CPU, і навмисно тими самими формулами:
// оракул кроку — збіг кількості намальованих патчів між двома незалежними
// шляхами, і збіг цей має бути точним, а не «в межах допуску». Тому кожен
// рядок нижче має пару в Rust, і розходження між ними — помилка, а не
// властивість заліза.
//
// ## Що лишається на CPU
//
// Вибір рівня, вирівнювання й маски зшивання: вони будують набір, а набір
// потрібен цілим ще до відбору (див. `cull.rs` — маска рахується з того, що
// лежить у наборі, тож викидати сусідів під час обходу не можна). Сюди
// приїжджає готовий список кандидатів, а звідси виходить його підмножина.
//
// ## Порядок вижилих недетермінований, і це свідомо
//
// Компактація йде атомарним лічильником, тож порядок залежить від того, які
// групи впорались першими. Для кадру це байдуже — патчі непрозорі й
// упорядковані глибиною, — а для звірки з CPU порівнюється **кількість**, не
// послідовність. Записано вголос, бо в цьому проєкті недетермінізм зазвичай
// помилка, і мовчазний виняток був би гіршим за сам виняток.

// Кандидат: слот геометрії в кеші й маска зшивання. Та сама пара, яку читає
// вершинна стадія — і це не збіг, а вимога: compute пише рівно те, що вона
// потім прочитає.
struct PatchDraw
{
    uint slot;
    uint mask;
};

// Конус патча на одиничній сфері, у системі ТІЛА.
struct Cone
{
    float3 axis;
    float cos_half;
    float sin_half;
};

struct CullParams
{
    // xyz — напрямок на камеру в системі тіла, w — косинус лімба.
    float4 to_eye;
    // Рядки повороту вигляду: світові осі → камерні.
    float4 view_right;
    float4 view_up;
    float4 view_back;
    // x, y — тангенси півкутів кадру; z, w — нормувальники площин.
    float4 frustum;
    // x — скільки кандидатів, решта — нулі.
    uint4 counts;
    // x — радіус тіла в метрах.
    float4 body;
};

ConstantBuffer<CullParams> params;

StructuredBuffer<PatchDraw> candidates;
StructuredBuffer<Cone> cones;
// Початок патча відносно камери, у світових осях — він же центр
// обмежувальної сфери. Той самий буфер, що читає вершинна стадія.
//
// ⚠ Структура мусить збігатися з `PatchData` у `patch.slang` **полем у поле**:
// це один буфер, і крок у ньому один. Спільного оголошення між двома
// шейдерами немає, тож копії дві — рівно як `SIDE` і `node_of`, і з тим самим
// висновком: правити треба обидві.
//
// Розбіжність тут не тиха: `gpu_driven::the_gpu_draws_exactly_as_many_patches_as_the_cpu_kept`
// падає першим же кадром, бо зсунутий крок дає чужий центр, а з ним і чужий
// відбір. Так вона й знайшлася на R7a.
struct PatchData
{
    float3 origin;
    uint tile;
    float2 window_origin;
    float window_step;
    float window_delta;
    // Колірне вікно (T3b). Відбір його не читає — воно тут рівно заради
    // **кроку** буфера, і саме тому порядок полів мусить бути той самий, що
    // в `patch.slang`: `float2` після скалярів, інакше std430 додасть дірку.
    uint colour_tile;
    float colour_step;
    float2 colour_origin;
};

StructuredBuffer<PatchData> origins;

RWStructuredBuffer<PatchDraw> visible;
// Аргументи `draw_indirect`: [vertex_count, instance_count, first_vertex,
// first_instance]. Другий елемент — і є лічильник вижилих.
RWStructuredBuffer<Atomic<uint>> indirect;

[shader("compute")]
[numthreads(64, 1, 1)]
void cull_main(uint3 id : SV_DispatchThreadID)
{
    uint index = id.x;
    if (index >= params.counts.x)
    {
        return;
    }

    PatchDraw candidate = candidates[index];
    Cone cone = cones[candidate.slot];

    // Горизонт: найбільше, на що патч здатний, — це cos(β − α), розкладене в
    // cos β cos α + sin β sin α. Дослівно `cull::beyond_limb`, разом із
    // окремим випадком «око всередині конуса»: там косинус парний і формула
    // повернула б кут до КРАЮ шапки замість нуля, тобто викинула б патч,
    // над яким стоїть камера. Правити треба обидві копії разом.
    float cos_beta = clamp(dot(cone.axis, params.to_eye.xyz), -1.0, 1.0);
    if (cos_beta < cone.cos_half)
    {
        float sin_beta = sqrt(max(1.0 - cos_beta * cos_beta, 0.0));
        float best = cos_beta * cone.cos_half + sin_beta * cone.sin_half;
        if (best <= params.to_eye.w)
        {
            return;
        }
    }

    // Frustum: обмежувальна сфера патча проти чотирьох бічних площин.
    // Центр — початок патча (він лежить на осі конуса), радіус — хорда до
    // найдальшого вузла. Дослівно `cull::bounding_sphere` і `cull::frustum`.
    float3 centre = origins[candidate.slot].origin;
    float radius = params.body.x * sqrt(max(2.0 - 2.0 * cone.cos_half, 0.0));

    float3 view = float3(
        dot(centre, params.view_right.xyz),
        dot(centre, params.view_up.xyz),
        dot(centre, params.view_back.xyz));

    float tx = params.frustum.x;
    float ty = params.frustum.y;
    bool outside =
        (view.x + tx * view.z) * params.frustum.z > radius ||
        (-view.x + tx * view.z) * params.frustum.z > radius ||
        (view.y + ty * view.z) * params.frustum.w > radius ||
        (-view.y + ty * view.z) * params.frustum.w > radius;
    if (outside)
    {
        return;
    }

    // `Atomic<uint>`, а не звичайний `uint`: у WGSL атомарність — властивість
    // ТИПУ, і без неї Slang просто не компілює точку входу під цю ціль.
    uint slot = indirect[1].add(1u);
    visible[slot] = candidate;
}
