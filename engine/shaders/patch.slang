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
    // тобто `scale_m / radius_m`. y — множник «одиниця зберігання кольору →
    // сірий на екрані»; **нуль у ньому означає, що кольору немає**, і тоді
    // фрагмент лишається на `colour`. z — скільки каналів у колірному тайлі:
    // один (яскравість Місяця) чи чотири (колір Землі, T7g). w — запас.
    float4 terrain;
    // Процедурний детайл (R7c): x — радіус тіла в метрах, y — одиниця
    // зберігання нахилу (`tiles::SLOPE_UNIT`), z — довжина хвилі найгрубішої
    // октави (`detail::base_m`, тобто радіус тіла поділений на сталу, а НЕ
    // глибина піраміди: та є станом кукера), w — пікселів на радіан
    // (`lod::focal_px`).
    float4 detail;
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
    // Вікно цього патча в його тайлі (R7a): зсув у вузлах і крок — скільки
    // вузлів тайла припадає на один вузол патча.
    //
    // Патч, що має власний тайл, дає `(0, 0)` і `1`, тобто точний `Load`, як
    // було до R7a. Патч, глибший за піраміду, дивиться в підпрямокутник
    // предка, і крок стає дробовим — звідси білінійна вибірка нижче.
    //
    // Числа рахує `Terrain::window` на CPU, тим самим кодом, що й
    // `Terrain::height_m`. Другого обчислення тут немає навмисно: два
    // незалежні вікна розійшлися б, і різницю було б видно тріщиною.
    float2 window_origin;
    float window_step;
    // Вирівнювання, і нічого більше. До етапу W тут жив крок центральної
    // різниці; нахил тепер лежить у самому тайлі. Слово лишилось, бо std430
    // однаково вимагає восьмибайтового вирівнювання під `colour_origin`:
    // структура без нього має ті самі 48 байтів, лише з дірою.
    float pad;
    // Те саме, але в **колірній** піраміді (T3b): номер тайла, зсув у вузлах
    // і крок.
    //
    // ⚠ Друге вікно, а не спільне з висотою, і це не дублювання: піраміди
    // різної глибини (5 рівнів висоти проти 6 кольору), тож той самий патч
    // цілком законно читає висоту в предка й колір — у власному тайлі.
    // Спільне вікно означало б, що глибини мусять збігтися.
    // ⚠ Порядок саме такий — `float2` **після** двох скалярів, — і це не смак.
    // У std430 `vec2<f32>` вимагає восьмибайтового вирівнювання, тож пара
    // «uint, float2, float» лягла б із діркою на четвертому слові, і структура
    // виросла б з 48 байтів до 64. Перевіряється це не міркуванням, а
    // згенерованим WGSL: `@align` кожного поля мусить збігатися з тим, як його
    // пакує `frame.rs`.
    uint colour_tile;
    float colour_step;
    float2 colour_origin;
};

StructuredBuffer<PatchData> patches;

// Тайли рельєфу — **bindless-масив текстур**, з першої ж текстури в рушії
// (правило 6 етапу R). Семплера поруч немає, і це не пропуск: тайл лежить
// вузол у вузол із сіткою патча, тож вибірка тут — точний `Load` за цілим
// індексом, а не фільтрація.
//
// Два канали, і обидва в одному текселі: `x` — висота, `y` — нахил, запечений
// кукером (етап W, Q3). Розділяти їх на дві текстури не можна навіть у думці —
// D19 платить за КІЛЬКІСТЬ текстур у масиві, тобто другий масив коштував би
// стільки ж, скільки весь рельєф.
//
// ⚠ Окрема група прив'язки (set 1), і це вимога wgpu, а не смак: «bind groups
// may not contain both a binding array and a dynamically offset buffer». А
// динамічний зсув у групі 0 — це вибір проходу глибини (R4a), тобто те, що
// нікуди не подінеться. Отже масив живе окремо.
[[vk::binding(0, 1)]]
Texture2D<int2> tiles[];

// Колір тієї самої поверхні — другий масив у тій самій групі (T3b).
//
// Семплера немає й тут, хоч кольору фільтрація потрібна: вибірка робиться
// вручну, чотирма `Load`, рівно як у `sample_tile`. Причина не в економії —
// **фільтрація семплером не знає про вікно**. Патч, глибший за колірну
// піраміду, читає підпрямокутник тайла предка, і координати текселя для нього
// рахує `colour_origin`/`colour_step`; UV для семплера довелося б виводити з
// тих самих чисел, тобто писати ту саму арифметику двічі.
[[vk::binding(1, 1)]]
Texture2D<float4> colours[];

// Геометрія патча: зсув вершини й нормаль, обидва на ОДИНИЧНІЙ сфері.
//
// Storage-буфер, а не вершинні атрибути (R6a). Причина одна й вона вся тут:
// зшивання рівнів — це підміна ІНДЕКСУ вузла, а вершинні атрибути приходять
// уже вибраними, тож підміну доводилось робити індексним буфером, тобто мати
// шістнадцять наборів і виклик малювання на патч. Читаючи вузол самотужки,
// шейдер робить ту саму підміну арифметикою — і патчі зливаються в один
// виклик з інстансами.
struct PatchVertex
{
    float3 offset;
    float3 normal;
};

StructuredBuffer<PatchVertex> vertices;

// Що малює цей інстанс: слот геометрії в кеші й маска зшивання.
struct PatchDraw
{
    uint slot;
    uint mask;
};

StructuredBuffer<PatchDraw> draws;

// Сітка патча. Мусить збігатися з `cubesphere::SIDE`; звіряє це тест
// `engine::tests::gpu_driven`, бо константа тут і константа там — два місця.
static const uint SIDE = 32;
static const uint NODES = SIDE + 1;

// Процедурний детайл (R7c). Числа — дослівно `engine::detail`; звіряє їх
// `engine::tests::detail`, бо константа тут і константа там — два місця.
static const uint OCTAVES = 6u;
static const float STEEPNESS = 0.5;
static const float FADE_LO_PX = 4.0;
static const float FADE_HI_PX = 16.0;

// Правило матеріалу (T4). Числа — дослівно `engine::material`; звіряє їх
// `engine::tests::material`, бо константа тут і константа там — два місця.
static const float SLOPE_GAIN = 0.30;
static const float SLOPE_REF = 0.15;
static const float RELIEF_GAIN = 0.45;
static const float MIN_TINT = 0.35;
static const float MAX_TINT = 1.80;

struct VertexInput
{
    uint vertex : SV_VertexID;
    uint instance : SV_InstanceID;
};

// Розкладений вузол: адреса в тайлі й індекс у буфері геометрії.
struct Node
{
    uint2 grid;   // (a, b) після підміни зшивання
    uint index;   // зміщення в буфері геометрії
};

// Вузол за номером вершини в списку трикутників патча.
//
// Порядок відтворює `cubesphere::indices` буквально: клітинка `(a, b)` з `a`
// зовнішнім циклом, два трикутники `v00 v10 v01` і `v01 v10 v11`. Розійтися
// їм не можна — тест звіряє кадр із тим, що малював індексний буфер, бітово.
Node node_of(uint vertex, uint slot, uint mask)
{
    uint triangle = vertex / 3u;
    uint corner = vertex % 3u;
    uint cell = triangle / 2u;
    uint half = triangle % 2u;

    uint a = cell / SIDE;
    uint b = cell % SIDE;

    // (da, db) кутів обох трикутників клітинки.
    uint2 first[3] = { uint2(0, 0), uint2(1, 0), uint2(0, 1) };
    uint2 second[3] = { uint2(0, 1), uint2(1, 0), uint2(1, 1) };
    uint2 step = half == 0u ? first[corner] : second[corner];
    a += step.x;
    b += step.y;

    // Зшивання: на ребрі з грубішим сусідом непарний вузол не існує, і всюди,
    // де він згаданий, стоїть попередній парний. Та сама підміна, що в
    // `cubesphere::indices`, і саме тому вона мусить бути тут дослівно.
    bool odd_on_b = (a % 2u == 1u)
        && ((b == 0u && (mask & 4u) != 0u) || (b == SIDE && (mask & 8u) != 0u));
    bool odd_on_a = (b % 2u == 1u)
        && ((a == 0u && (mask & 1u) != 0u) || (a == SIDE && (mask & 2u) != 0u));
    if (odd_on_b) { a -= 1u; }
    if (odd_on_a) { b -= 1u; }

    Node out;
    out.grid = uint2(a, b);
    out.index = slot * NODES * NODES + a * NODES + b;
    return out;
}

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
    // Позиція в світових осях відносно камери — потрібна рельєфу, щоб
    // фрагмент узяв із неї геометричну нормаль.
    float3 world : WORLD;
    // Та сама позиція **до зсуву рельєфом**, тобто на гладкій сфері.
    //
    // Потрібна рівно для одного: фасетка, порахована з `world`, містить і
    // нахил рельєфу, і злам самої теселяції сфери, а другого в нормалі бути
    // не має. Фасетка з `base` — це той злам сам по собі, тож різниця двох
    // фасеток лишає чистий нахил рельєфу (T7f).
    float3 base : BASE;
    // Де цей піксель лежить у колірному тайлі: номер тайла й координата в
    // його текселях (T3b).
    //
    // Тайл — `nointerpolation`: він цілий і той самий на весь патч, а
    // інтерполяція цілого між вершинами дала б номер чужого тайла на
    // більшості пікселів. Координата, навпаки, інтерполюється — саме вона й
    // робить колір неперервним усередині клітинки.
    nointerpolation uint colour_tile : COLOURTILE;
    float2 colour_node : COLOURNODE;
    // Множник альбедо від нахилу й процедурного рельєфу (T4).
    //
    // Рахується у ВЕРШИНІ, і це рішення, а не зручність: обидва його аргументи
    // на спільному вузлі двох патчів різних рівнів бітово одні, тож колір на
    // межі рівнів неперервний за побудовою. Порахований у фрагменті з
    // `ddx/ddy`, він був би функцією трикутника, тобто рівня патча.
    float tint : TINT;
};

// Спільна частина: зсув вершини всередині патча, вже повернутий і масштабований.
VertexOutput place(uint slot, PatchVertex vertex, uint2 grid, float3 offset)
{
    PatchData patch = patches[slot];
    float3 origin = patch.origin;
    float3 world = origin + mul(uniforms.model, float4(offset, 0.0)).xyz;

    VertexOutput output;
    // Одиниця — «правило нічого не робить». Гладке тіло лишає її як є, а
    // рельєф перезаписує; на рівнині рельєф теж дає рівно одиницю.
    output.tint = 1.0;
    output.colour_tile = patch.colour_tile;
    // Вузол патча → тексель колірного тайла, тим самим вікном, що й у висоти,
    // але **своїм**. Зсуву на ореол тут немає з етапу W: обидва тайли — рівно
    // сітка патча.
    output.colour_node = patch.colour_origin + float2(grid) * patch.colour_step;
    output.position = mul(uniforms.projection, float4(world, 1.0));
    // Той самий поворот, що й для зсуву: нормаль, повернута інакше, ніж
    // геометрія, дає освітлення чужого тіла. Радіус у матриці нормаль лише
    // подовжує — `normalize` у фрагменті його прибирає.
    output.normal = mul(uniforms.model, float4(vertex.normal, 0.0)).xyz;
    output.world = world;
    // Незсунута вершина того самого патча. Гладкий прохід кладе сюди те саме,
    // що й у `world`, — і саме тому різниця фасеток у нього тотожно нульова.
    output.base = origin + mul(uniforms.model, float4(vertex.offset, 0.0)).xyz;
    return output;
}

[shader("vertex")]
VertexOutput vertex_smooth(VertexInput input)
{
    PatchDraw draw = draws[input.instance];
    Node node = node_of(input.vertex, draw.slot, draw.mask);
    PatchVertex vertex = vertices[node.index];
    return place(draw.slot, vertex, node.grid, vertex.offset);
}

// Висота й нахил у вузлі патча, в одиницях зберігання (R7a; етап W).
//
// Дослівний двійник `Terrain::sample` — рівно так само, як `cull.slang` є
// двійником `engine::cull`. Оракул той самий: числа з обох боків мусять
// збігтися, і тест їх звіряє.
//
// **Одна вибірка на два числа.** Висота й нахил лежать у тому самому текселі, а
// вікно в них одне, тож чотири `Load` дають обидва. До етапу W нахил рахувався
// тут центральною різницею — чотири вибірки по чотири `Load`, тобто двадцять
// на вузол проти теперішніх чотирьох, — і на куті куба давав три різні
// відповіді залежно від грані (Q3).
//
// Тайл лежить вузол у вузол із сіткою патча, і `a` в ньому — рядок, тобто
// `y` текстури. Коли патч має власний тайл, `window_step == 1`, зсув нульовий,
// і всі чотири відліки падають в один вузол — вибірка вироджується в точний
// `Load`, який тут і був до R7a.
float2 sample_tile(PatchData patch, uint2 grid)
{
    float x = patch.window_origin.x + float(grid.x) * patch.window_step;
    float y = patch.window_origin.y + float(grid.y) * patch.window_step;

    float x0 = floor(x);
    float y0 = floor(y);
    float tx = x - x0;
    float ty = y - y0;

    // Затискання по верхньому вузлу: правий і нижній краї тайла не мають
    // сусіда всередині нього. Крок ділить `SIDE` націло, тож затиснута
    // вибірка трапляється рівно на межі, де `tx` або `ty` вже нульові, і на
    // результат не впливає.
    int xi = int(x0);
    int yi = int(y0);
    int xi1 = min(xi + 1, int(SIDE));
    int yi1 = min(yi + 1, int(SIDE));

    // `Load` бере (x, y, mip) текстури = (b, a, 0): `b` — стовпець, `a` — рядок.
    // Зсуву на ореол тут немає: тайл рельєфу — це рівно сітка патча.
    float2 h00 = float2(tiles[patch.tile].Load(int3(yi, xi, 0)).xy);
    float2 h01 = float2(tiles[patch.tile].Load(int3(yi1, xi, 0)).xy);
    float2 h10 = float2(tiles[patch.tile].Load(int3(yi, xi1, 0)).xy);
    float2 h11 = float2(tiles[patch.tile].Load(int3(yi1, xi1, 0)).xy);

    float2 top = h00 * (1.0 - ty) + h01 * ty;
    float2 bottom = h10 * (1.0 - ty) + h11 * ty;
    return top * (1.0 - tx) + bottom * tx;
}

// Хеш трьох цілих у [0, 1) — двійник `engine::detail::hash`.
float detail_hash(int x, int y, int z)
{
    uint h = uint(x) * 0x9E3779B1u ^ uint(y) * 0x85EBCA6Bu ^ uint(z) * 0xC2B2AE35u;
    h ^= h >> 15u;
    h = h * 0x2545F491u;
    h ^= h >> 13u;
    h = h * 0x27D4EB2Fu;
    h ^= h >> 16u;
    return float(h >> 8u) / 16777216.0;
}

float detail_smooth(float t)
{
    return t * t * (3.0 - 2.0 * t);
}

// Значеннєвий шум — двійник `engine::detail::value_noise`.
float value_noise(float3 p)
{
    float3 cell = floor(p);
    float3 t = float3(detail_smooth(p.x - cell.x),
                      detail_smooth(p.y - cell.y),
                      detail_smooth(p.z - cell.z));
    int x = int(cell.x);
    int y = int(cell.y);
    int z = int(cell.z);

    float out = 0.0;
    for (int dx = 0; dx < 2; ++dx)
    {
        float wx = dx == 0 ? 1.0 - t.x : t.x;
        for (int dy = 0; dy < 2; ++dy)
        {
            float wy = dy == 0 ? 1.0 - t.y : t.y;
            for (int dz = 0; dz < 2; ++dz)
            {
                float wz = dz == 0 ? 1.0 - t.z : t.z;
                out += detail_hash(x + dx, y + dy, z + dz) * wx * wy * wz;
            }
        }
    }
    return out;
}

// Вага октави — двійник `engine::detail::octave_weight`.
//
// Залежить від довжини хвилі, відстані й фокуса, і НІ ВІД ЧОГО більше.
// Зокрема не від рівня патча: сусіди різняться на рівень за побудовою, і
// критерій, що дивиться на рівень, дав би на спільному вузлі дві висоти.
float octave_weight(float wavelength, float distance, float focal)
{
    float px = wavelength / max(distance, 1.0) * focal;
    if (px <= FADE_LO_PX) { return 0.0; }
    if (px >= FADE_HI_PX) { return 1.0; }
    return detail_smooth((px - FADE_LO_PX) / (FADE_HI_PX - FADE_LO_PX));
}

// Процедурна деталь у вузлі — двійник `engine::detail::sample`.
//
// Повертає два числа з одного проходу: `x` — висота в метрах для форми,
// `y` — шорсткість без розмірності для матеріалу (T4). Шум, точки вибірки й
// затухання спільні; різниця лише у вазі октави — висота множить кожну на її
// довжину хвилі, шорсткість не множить ні на що. Другий прохід коштував би
// стільки ж, скільки перший: усе дороге тут — це хеші.
//
// `unit` — одиничний напрямок вузла з центра тіла, тобто нормаль з буфера
// геометрії. Саме вона, а не позиція відносно патча: на спільному ребрі
// нормаль бітово одна (R2b), тож деталь неперервна не за домовленістю, а за
// побудовою.
float2 detail_sample(float3 unit, float slope, float distance)
{
    float radius = uniforms.detail.x;
    float base = uniforms.detail.z;
    float focal = uniforms.detail.w;

    float height = 0.0;
    float roughness = 0.0;
    for (uint octave = 0u; octave < OCTAVES; ++octave)
    {
        float wavelength = base / float(1u << octave);
        float weight = octave_weight(wavelength, distance, focal);
        if (weight <= 0.0) { break; }
        float scale = radius / wavelength;
        float signed_noise = value_noise(unit * scale) - 0.5;
        height += signed_noise * STEEPNESS * slope * wavelength * weight;
        roughness += 2.0 * signed_noise * weight;
    }
    return float2(height, roughness);
}

// Множник альбедо у вузлі — двійник `engine::material::tint`.
//
// Жоден з двох аргументів не походить від ассета: нахил — величина в метрах
// на метр, шорсткість — шум за позицією на тілі. Крокові піраміди сюди немає
// куди зайти, і саме тому перекукування ассета не перефарбовує схили.
// Третій аргумент — висота вузла в одиницях зберігання. Нижче за рівень моря
// правило віддає рівно одиницю: воно підсвічує схил, а під водою в кадрі видно
// не схил дна, а поверхню моря. На Землі дно виміряно **крутішим** за сушу,
// тож без цієї гілки правило малювало б серединні хребти поверх рівної води.
float material_tint(float slope, float roughness, float height)
{
    if (height < uniforms.terrain.w)
    {
        return 1.0;
    }
    float steep = clamp(slope / SLOPE_REF, 0.0, 1.0);
    return clamp(1.0 + steep * (SLOPE_GAIN + RELIEF_GAIN * roughness), MIN_TINT, MAX_TINT);
}

[shader("vertex")]
VertexOutput vertex_terrain(VertexInput input)
{
    PatchDraw draw = draws[input.instance];
    Node node = node_of(input.vertex, draw.slot, draw.mask);
    PatchVertex vertex = vertices[node.index];
    PatchData patch = patches[draw.slot];

    // Два числа з однієї вибірки: `x` — висота в одиницях зберігання, `y` —
    // нахил у своїх (`tiles::SLOPE_UNIT`).
    float2 tile = sample_tile(patch, node.grid);
    float height = tile.x;
    float slope = tile.y * uniforms.detail.y;

    // Відстань до камери береться по НЕЗСУНУТІЙ вершині, і це не недогляд:
    // `origin` уже camera-relative, тож довжина суми і є відстанню. Рахувати
    // її після зсуву означало б залежність деталі від самої себе, а виграшу
    // не дало б — деталь це метри проти сотень.
    //
    // По вершині, а не по патчу: `length(origin)` було б однакове для всього
    // патча, тобто вага октави стала б функцією патча, а не позиції — і два
    // сусіди різних рівнів розійшлися б на спільному вузлі (правило 3).
    float3 base_world = patch.origin + mul(uniforms.model, float4(vertex.offset, 0.0)).xyz;
    float distance = length(base_world);

    float2 detail = detail_sample(vertex.normal, slope, distance);

    // Обидві висоти зсувають вершину вздовж нормалі патча — на ОДИНИЧНІЙ
    // сфері, бо радіус і поворот застосовує та сама матриця, що й до самого
    // зсуву. Множник `terrain.x` і є `scale_m / radius_m`; деталь приходить
    // одразу в метрах, тож ділиться на радіус.
    float lift = height * uniforms.terrain.x + detail.x / uniforms.detail.x;
    float3 offset = vertex.offset + vertex.normal * lift;
    VertexOutput output = place(draw.slot, vertex, node.grid, offset);
    // Ті самі два числа, якими щойно зсунуто вершину, — інших правило не
    // читає. Тобто колір не може розійтися з формою: він виведений з неї.
    output.tint = material_tint(slope, detail.y, height);
    return output;
}

// Одинична нормаль фасетки, розвернута назовні.
//
// Похідні дають вектор з довільним знаком — розвертає його нормаль сфери, яка
// завжди правильна за напрямком. Вироджений добуток (трикутник у точку на
// екрані) віддає саму нормаль сфери, а не NaN: ділити на нуль тут нема потреби,
// а `normalize(0)` пролізло б у кадр чорною крапкою.
float3 outward(float3 cross_product, float3 sphere)
{
    float length_squared = dot(cross_product, cross_product);
    if (length_squared < 1e-30)
    {
        return sphere;
    }
    float3 unit = cross_product * rsqrt(length_squared);
    return dot(unit, sphere) < 0.0 ? -unit : unit;
}

float3 shade(float3 normal, float3 albedo)
{
    float3 n = normalize(normal);
    float diffuse = max(dot(n, uniforms.light_dir.xyz), 0.0);
    return albedo * (0.05 + 0.95 * diffuse);
}

// Колір у точці колірного тайла, білінійно між чотирма текселями (T3b).
//
// Та сама форма, що `sample_tile`, і та сама причина: вікно вже перевело
// вузол патча в тексель **цього** тайла, а між текселями треба інтерполювати
// самим — семплер про вікно не знає.
//
// Затискання по обох осях до `[0, NODES − 1]`: у вузлі на самому краю патча
// правий сусід уже поза текстурою, а вага його там нульова.
float3 sample_colour(uint tile, float2 node)
{
    float top = float(NODES - 1u);
    float x = clamp(node.x, 0.0, top);
    float y = clamp(node.y, 0.0, top);

    int xi = int(floor(x));
    int yi = int(floor(y));
    float tx = x - float(xi);
    float ty = y - float(yi);
    int xi1 = min(xi + 1, int(top));
    int yi1 = min(yi + 1, int(top));

    // `a` — рядок тайла, тобто `y` текстури: той самий порядок, що в
    // `sample_tile`, і переставити його тут означало б транспонувати колір
    // відносно рельєфу.
    float3 c00 = colours[tile].Load(int3(yi, xi, 0)).rgb;
    float3 c01 = colours[tile].Load(int3(yi1, xi, 0)).rgb;
    float3 c10 = colours[tile].Load(int3(yi, xi1, 0)).rgb;
    float3 c11 = colours[tile].Load(int3(yi1, xi1, 0)).rgb;

    float3 left = c00 * (1.0 - ty) + c01 * ty;
    float3 right = c10 * (1.0 - ty) + c11 * ty;
    return left * (1.0 - tx) + right * tx;
}

// Колір поверхні в цьому пікселі: тайл, якщо він є, інакше колір тіла.
//
// Гілка за uniform, і саме у **фрагментній** стадії, де вона безпечна: пастка
// ACO з F6 стосувалася вершинної (той самий uniform там читався неправильно),
// а фрагментна читала його коректно — це перевірялося виводом значення в
// колір. Друга точка входу тут не потрібна: гладке тіло малюється своїм
// пайплайном і в цю функцію не заходить.
// Множник матеріалу застосовується в **обох** гілках, і це навмисно: правило
// описує поверхню, а не наявність мозаїки. Тіло з рельєфом, але без кольору,
// дістає той самий рельєфний контраст — інакше поява ассета міняла б не лише
// відтінок, а й саму присутність деталі.
float3 surface_albedo(VertexOutput input)
{
    if (uniforms.terrain.y <= 0.0)
    {
        return uniforms.colour.rgb * input.tint;
    }
    float3 unit = sample_colour(input.colour_tile, input.colour_node);
    // Одноканальний тайлсет (Місяць) несе яскравість, і читається з нього
    // рівно перший канал — решта в такій текстурі нулі. Чотириканальний
    // (Земля) несе колір; sRGB розкодовує апаратура при читанні текселя, тож
    // сюди він приходить уже лінійним.
    float3 albedo = uniforms.terrain.z >= 2.0 ? unit : unit.xxx;
    return albedo * uniforms.terrain.y * input.tint;
}

[shader("fragment")]
float4 fragment_smooth(VertexOutput input) : SV_Target
{
    return float4(shade(input.normal, uniforms.colour.rgb), 1.0);
}

[shader("fragment")]
float4 fragment_terrain(VertexOutput input) : SV_Target
{
    // Геометрична нормаль фасетки: рельєф це набір площинок розміром із
    // клітинку DEM, і аналітична нормаль сфери про них не знає нічого.
    //
    // ⚠ Але сама фасетка знає про сферу зайве. Похідні беруться по трикутнику,
    // тобто фасетка стала на весь трикутник — разом із кривиною сфери, яку
    // теселяція теж ламає на площинки. Здалеку, коли клітинка сітки більша за
    // рельєф під нею, у кадрі лишається саме той злам: **виміряно на T7f** —
    // Земля з 10⁶ м виходила смугастою плитками по 135 пікселів зі стрибком
    // яскравості 3%, і смуги збігалися з трикутниками, а не з патчами.
    //
    // Тому з фасетки віднімається фасетка **тієї самої** сітки до зсуву:
    // злам теселяції в них однаковий і скорочується, а лишається чистий нахил
    // рельєфу, доданий до аналітичної нормалі сфери. На гладкому тілі різниця
    // тотожно нульова, тобто правило нічого не робить, — той самий принцип, що
    // й у множника матеріалу.
    // ⚠ Нормаль сфери **нормалізується тут**, і це не косметика: `model` — це
    // поворот, помножений на радіус тіла, тож `input.normal` приходить
    // завдовжки з радіус. Поправка завдовжки 0.05 у сумі з вектором завдовжки
    // 1.7·10⁶ зникає без сліду — саме так ця правка з першого разу й вимкнула
    // рельєф цілком, зрівнявши кадр з гладкою сферою до одиниці.
    float3 sphere = normalize(input.normal);
    float3 facet = outward(cross(ddx(input.world), ddy(input.world)), sphere);
    float3 tessellation = outward(cross(ddx(input.base), ddy(input.base)), sphere);
    float3 normal = normalize(sphere + facet - tessellation);
    return float4(shade(normal, surface_albedo(input)), 1.0);
}
