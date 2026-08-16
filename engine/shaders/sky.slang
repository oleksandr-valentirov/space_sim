// Повітря за моделлю Hillaire 2020 (PROJECT.md §7, ROADMAP-ATMOSPHERE.md).
//
// Дослівний двійник `engine::atmosphere` — рівно так само, як `cull.slang` є
// двійником `engine::cull`. Правити треба обидва: оракул кроку — збіг чисел з
// обох боків, і розійшовшись, вони перестають бути перевіркою одне одного.
//
// Одиниці — **метри й 1/м**, як у решті рушія. У статті все в кілометрах, і це
// рівно те перетворення, на якому найлегше мовчки загубити три порядки (S1).

// Параметри повітря — те саме, що `engine::scene::Atmosphere`, спаковане в
// чотири вектори. Пакування, а не поле в полі: constant buffer у std140
// однаково вирівняв би кожен `float3` на 16 байтів, тож щільна упаковка тут
// нічого не приховує, а розкладку робить очевидною.
struct AirParams
{
    // rgb — розсіювання Релея на рівні поверхні, 1/м; w — його висота шкали, м.
    float4 rayleigh;
    // x — розсіювання Мі, y — поглинання Мі (обидва 1/м), z — висота шкали,
    // w — асиметрія фазової функції.
    float4 mie;
    // rgb — поглинання озоном на піку, 1/м; w — не використовується.
    float4 ozone;
    // x — центр озонового шару, y — його півширина (обидва м),
    // z — радіус поверхні тіла, w — радіус верхньої межі повітря.
    float4 shape;
};

ConstantBuffer<AirParams> air;

// Таблиця пропускання: 256×64, rgba16float (S2).
//
// Розміри продубльовані в `engine::atmosphere::TRANSMITTANCE_WIDTH/HEIGHT`, і
// спільної константи між Rust і Slang не існує — тест греппить цей файл, як
// уже робить із `SIDE` у патчів.
static const uint TRANSMITTANCE_WIDTH = 256u;
static const uint TRANSMITTANCE_HEIGHT = 64u;

// Скільки кроків робить інтегрування ТУТ.
//
// П'ятсот, і число виміряне, а не взяте зі статті (там сорок). Сорока
// вистачає всюди, крім променів, що виходять від самої землі під малим кутом:
// там шлях у 560 км ділиться на кроки по 14 км при висоті шкали 8 км, і
// правило середньої точки помиляється на 5·10⁻³ — увосьмеро більше за крок
// зберігання в half-float. П'ятсот дають 3.6·10⁻⁵, тобто впираються вже у
// формат, а не в арифметику. Платиться це раз на набір параметрів повітря, не
// раз на кадр, тож дешевше було б лише неправильно.
static const uint TRANSMITTANCE_STEPS = 500u;

// `WTexture2D` (лише запис), а не `RWTexture2D`: останній компілюється у
// `texture_storage_2d<..., read_write>`, а WebGPU дозволяє читання-запис лише
// для одноканальних 32-бітних форматів. Формат оголошений тут же, бо WGSL
// вимагає його в типі — вивести його з боку Rust неможливо.
[format("rgba16f")]
WTexture2D<float4> transmittance_out;

// Густина трьох компонент на висоті `h` над поверхнею — двійник
// `atmosphere::density`.
//
// ⚠ Форма озонового шару лежить у `shape`, а НЕ в `ozone`: у `ozone` самі
// коефіцієнти поглинання. Переплутати їх легко — обидва вектори звуться про
// озон, — і коштувало це вже одного разу: шар виходив завширшки два
// мікрометри, тобто озону не було зовсім. Помітно це лише на пологому промені
// в стратосфері, де озон і працює; у зеніті різниця тоне у похибці.
float3 density(float h)
{
    float rayleigh = exp(-h / air.rayleigh.w);
    float mie = exp(-h / air.mie.z);
    float ozone = max(0.0, 1.0 - abs(h - air.shape.x) / air.shape.y);
    return float3(rayleigh, mie, ozone);
}

// Ослаблення на висоті `h`, 1/м по RGB — двійник `atmosphere::extinction`.
float3 extinction(float h)
{
    float3 d = density(h);
    float mie = air.mie.x + air.mie.y;
    return air.rayleigh.rgb * d.x + float3(mie, mie, mie) * d.y + air.ozone.rgb * d.z;
}

// Скільки метрів до верхньої межі — двійник `atmosphere::distance_to_top`.
float distance_to_top(float r, float mu, float top)
{
    float discriminant = r * r * (mu * mu - 1.0) + top * top;
    return max(0.0, -r * mu + sqrt(max(0.0, discriminant)));
}

// Скільки метрів до поверхні; від'ємне — поверхні попереду немає. Двійник
// `atmosphere::distance_to_ground`, який там повертає `Option`.
float distance_to_ground(float r, float mu, float bottom)
{
    float discriminant = r * r * (mu * mu - 1.0) + bottom * bottom;
    if (mu >= 0.0 || discriminant < 0.0) { return -1.0; }
    return -r * mu - sqrt(discriminant);
}

// Оптична товща на відрізку `span` уздовж `(r, mu)` — двійник
// `atmosphere::optical_depth`, правило середньої точки.
//
// ⚠ Відрізок приходить **параметром**, а не рахується тут із `(r, mu)`, і це
// не стиль. Кінець променя біля дотику до поверхні стоїть на різниці
// `r² − bottom²`, яка в самому дотику дорівнює нулю; у `f32` при `r ≈ 6.4·10⁶`
// одиниця останнього розряду квадрата — 4·10⁶, тобто знак тій різниці дає
// округлення, а не геометрія. Коштувало це на S2 шляху, обрізаного вдесятеро:
// крайній стовпець таблиці розходився з оракулом на 0.21 замість 10⁻³. Той,
// хто відрізок задає, майже завжди знає його точніше — параметризація таблиці,
// наприклад, дає його як `rho + H`, без жодного віднімання.
float3 optical_depth(float r, float mu, float span, uint steps)
{
    float bottom = air.shape.z;
    float step = span / float(steps);
    float3 sum = float3(0.0, 0.0, 0.0);
    for (uint k = 0u; k < steps; ++k)
    {
        float d = (float(k) + 0.5) * step;
        float h = sqrt(max(0.0, r * r + d * d + 2.0 * r * d * mu)) - bottom;
        sum += extinction(h) * step;
    }
    return sum;
}

// Точка `(r, mu)` за координатами текселя — двійник `atmosphere::uv_to_r_mu`.
//
// Важлива властивість одна: при `u = 0` виходить `mu = 1`, тобто промінь
// строго вгору, для якого існує замкнена форма. На ній стоїть увесь оракул S2.
//
// Третім виходом іде сама відстань до верхньої межі: параметризація її вже
// знає (це і є `d`), і повертати її дешевше й точніше, ніж відновлювати з
// `(r, mu)` — див. застереження в `optical_depth`.
void uv_to_r_mu(float2 uv, out float r, out float mu, out float d)
{
    float bottom = air.shape.z;
    float top = air.shape.w;

    float h = sqrt(max(0.0, top * top - bottom * bottom));
    float rho = h * uv.y;
    r = sqrt(rho * rho + bottom * bottom);

    float d_min = top - r;
    float d_max = rho + h;
    d = d_min + uv.x * (d_max - d_min);
    mu = d == 0.0 ? 1.0 : clamp((h * h - rho * rho - d * d) / (2.0 * r * d), -1.0, 1.0);
}

// Таблиця пропускання: тексель на пару `(r, mu)`, значення — `exp(−τ)`.
//
// Рахується раз на набір параметрів повітря, не раз на кадр: пропускання не
// залежить ні від камери, ні від Сонця (ROADMAP-ATMOSPHERE.md, таблиця в
// правилі 5).
[shader("compute")]
[numthreads(8, 8, 1)]
void transmittance_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= TRANSMITTANCE_WIDTH || id.y >= TRANSMITTANCE_HEIGHT) { return; }

    // Кінці діапазону — у центрах крайніх текселів, а не за межами текстури.
    //
    // Тобто тексель 0 несе рівно `u = 0`, тобто рівно `mu = 1` — вертикаль,
    // єдиний напрямок із замкненою формою. Ділити на ширину замість
    // `ширина − 1` означало б, що вертикалі в таблиці немає взагалі, а
    // найближчий до неї тексель має `mu = 0.98`: перевірити таблицю формулою
    // стало б ні на чому. Читач робить зворотне перетворення
    // (`atmosphere::unit_to_texture`), і саме тому білінійна вибірка на кінцях
    // не залазить за край.
    float2 uv = float2(float(id.x) / float(TRANSMITTANCE_WIDTH - 1u),
                       float(id.y) / float(TRANSMITTANCE_HEIGHT - 1u));
    float r;
    float mu;
    float span;
    uv_to_r_mu(uv, r, mu, span);

    float3 depth = optical_depth(r, mu, span, TRANSMITTANCE_STEPS);
    transmittance_out.Store(id.xy, float4(exp(-depth), 1.0));
}
