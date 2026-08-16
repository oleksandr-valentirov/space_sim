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

// ## Групи прив'язки, і чому їх дві
//
// Група 0 — те, що прохід **читає**; група 1 — те, що він **пише**. Поділ не
// косметичний: таблицю пропускання пише один прохід і читають усі наступні, а
// одна bind-група, у якій та сама текстура стоїть і на запис, і на читання,
// заборонена — wgpu бачить у ній гонку незалежно від того, що робить шейдер.
// Тому прохід пропускання дістає групу 0 без самої таблиці (макет, у якому цих
// прив'язок просто немає), а решта — повну.
//
// Слоти групи 1 не перетинаються: у кожного проходу свій. Макет кожного
// пайплайна оголошує рівно свій слот, і це дозволено — від макета вимагається
// накривати те, що точка входу читає, а не все, що є в модулі.
[[vk::binding(0, 0)]]
ConstantBuffer<AirParams> air;

// Один семплер на всі таблиці: білінійна фільтрація з затисканням по краях.
// Затискання, а не повтор, — і саме тому кінці одиничного діапазону сидять у
// центрах крайніх текселів: інакше половина крайнього текселя читалася б з
// затиснутого краю, а не з таблиці.
[[vk::binding(1, 0)]]
SamplerState lut_sampler;

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

// Таблиця багаторазового розсіювання: 32×32 (S3).
static const uint MULTISCATTER_SIZE = 32u;
// Скільки напрямків обходить інтегрування по сфері — сітка 8×8.
static const uint MULTISCATTER_DIRECTIONS = 64u;
// Скільки кроків робить промінь усередині одного напрямку.
static const uint MULTISCATTER_STEPS = 20u;

// Таблиця неба: 192×108 (S4).
static const uint SKYVIEW_WIDTH = 192u;
static const uint SKYVIEW_HEIGHT = 108u;
// Скільки кроків робить промінь таблиці неба.
static const uint SKYVIEW_STEPS = 32u;

static const float PI = 3.14159265358979;

// Таблиця пропускання на читання — для всіх проходів, крім того, що її пише.
[[vk::binding(2, 0)]]
Texture2D<float4> transmittance_lut;

// Таблиця багаторазового розсіювання на читання.
[[vk::binding(3, 0)]]
Texture2D<float4> multiscatter_lut;

// Те, що міняється **щокадру**: де камера й де Сонце.
//
// Окремим буфером від `air`, і це не дрібниця: параметри повітря живуть, доки
// живе тіло, а ці — один кадр. Одна структура на двох означала б, що зміна
// камери виглядає як зміна атмосфери, тобто перерахунок таблиць, яких ніхто не
// просив (правило 5 етапу S).
struct ViewParams
{
    // x — відстань камери від центра тіла, y — косинус зенітного кута Сонця
    // в точці камери. z, w — вирівнювання: менше за `float4` constant buffer
    // однаково не буває.
    float4 view;
};

[[vk::binding(4, 0)]]
ConstantBuffer<ViewParams> frame;

// `WTexture2D` (лише запис), а не `RWTexture2D`: останній компілюється у
// `texture_storage_2d<..., read_write>`, а WebGPU дозволяє читання-запис лише
// для одноканальних 32-бітних форматів. Формат оголошений тут же, бо WGSL
// вимагає його в типі — вивести його з боку Rust неможливо.
[[vk::binding(0, 1)]]
[format("rgba16f")]
WTexture2D<float4> transmittance_out;

[[vk::binding(1, 1)]]
[format("rgba16f")]
WTexture2D<float4> multiscatter_out;

[[vk::binding(2, 1)]]
[format("rgba16f")]
WTexture2D<float4> skyview_out;

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
    // Під поверхнею повітря немає, і затискання тут не косметика: промінь, що
    // пірнув під землю на 6371 км, дав би `exp(+796)`, тобто нескінченність, а
    // нескінченність, помножена на нульовий крок, дає NaN — і вся таблиця
    // виходить із нього. Спіймано на S3, на текселях рівня поверхні.
    h = max(h, 0.0);
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

// ⚠ **Змінна тут — `rho² = r² − bottom²`, а не радіус**, і етап S спіткнувся
// про це двічі. Обидві відстані виражаються через нього без жодного віднімання
// великих чисел; хто знає його точніше (параметризація таблиці, висота над
// поверхнею), той і мусить його передати. У `f32` при `r ≈ 6.4·10⁶` одиниця
// останнього розряду квадрата — 4·10⁶, тобто біля поверхні й біля дотику знак
// різниці дає округлення, а не геометрія.
//
// `rho2` — це `r² − bottom²` точки, `shell2` — `top² − bottom²` атмосфери.

// Скільки метрів до верхньої межі — двійник `atmosphere::distance_to_top`.
float distance_to_top(float r, float mu, float rho2, float shell2)
{
    float discriminant = r * r * mu * mu + (shell2 - rho2);
    return max(0.0, -r * mu + sqrt(max(0.0, discriminant)));
}

// Скільки метрів до поверхні; від'ємне — поверхні попереду немає. Двійник
// `atmosphere::distance_to_ground`, який там повертає `Option`.
float distance_to_ground(float r, float mu, float rho2)
{
    float discriminant = r * r * mu * mu - rho2;
    if (mu >= 0.0 || discriminant < 0.0) { return -1.0; }
    // `max(0)` — не косметика. Промінь, що йде вниз із самої поверхні, має
    // `rho² = 0`, і різниця `−r·mu − √(r²mu²)` у `f32` виходить то трохи
    // додатною, то трохи від'ємною. Від'ємну викликач читає як «поверхні
    // попереду немає» й веде промінь крізь планету — з тінню, порахованою
    // зсередини неї. Спіймано на S3: рядок 0 таблиці виходив утричі яскравішим.
    return max(0.0, -r * mu - sqrt(discriminant));
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

// ---------------------------------------------------------------------------
// Багаторазове розсіювання (S3)
// ---------------------------------------------------------------------------

// Координата текстури, у якій лежить одиничне значення `u` — двійник
// `atmosphere::unit_to_texture`. Кінці діапазону сідають у центри крайніх
// текселів; без цього білінійна вибірка на краю читала б затиснений тексель
// замість таблиці.
float unit_to_texture(float u, uint size)
{
    float n = float(size);
    return 0.5 / n + u * (1.0 - 1.0 / n);
}

// Одиничні координати таблиці пропускання за `(r, mu)` — двійник
// `atmosphere::r_mu_to_uv`.
float2 r_mu_to_uv(float r, float mu)
{
    float bottom = air.shape.z;
    float top = air.shape.w;

    float h = sqrt(max(0.0, top * top - bottom * bottom));
    float rho = sqrt(max(0.0, r * r - bottom * bottom));
    float d = distance_to_top(r, mu, rho * rho, h * h);
    float d_min = top - r;
    float d_max = rho + h;
    float u = d_max > d_min ? clamp((d - d_min) / (d_max - d_min), 0.0, 1.0) : 0.0;
    return float2(u, clamp(rho / h, 0.0, 1.0));
}

// Пропускання від `(r, mu)` до верхньої межі — з таблиці, білінійно.
float3 sample_transmittance(float r, float mu)
{
    float2 uv = r_mu_to_uv(r, mu);
    float2 coord = float2(unit_to_texture(uv.x, TRANSMITTANCE_WIDTH),
                          unit_to_texture(uv.y, TRANSMITTANCE_HEIGHT));
    return transmittance_lut.SampleLevel(lut_sampler, coord, 0.0).rgb;
}

// Розсіювання (без поглинання) на висоті `h` — двійник `atmosphere::scattering`.
//
// Не те саме, що `extinction`: озон не розсіює зовсім, Мі розсіює менше, ніж
// гасить. У джерелі стоїть саме розсіювання — з'їдений фотон у небо не летить.
float3 scattering(float h)
{
    float3 d = density(h);
    float mie = air.mie.x * d.y;
    return air.rayleigh.rgb * d.x + float3(mie, mie, mie);
}

// Напрямок номер `k` з рівномірної сітки 8×8 на сфері — двійник
// `atmosphere::sphere_direction`.
float3 sphere_direction(uint k)
{
    float i = 0.5 + float(k / 8u);
    float j = 0.5 + float(k % 8u);
    float theta = 2.0 * 3.14159265358979 * i / 8.0;
    float phi = acos(1.0 - 2.0 * j / 8.0);
    return float3(sin(phi) * cos(theta), sin(phi) * sin(theta), cos(phi));
}

// Таблиця багаторазового розсіювання: 32×32, вісь `u` — косинус зенітного кута
// Сонця, вісь `v` — висота над поверхнею. Лінійно по обох, і це не лінощі:
// нелінійність у таблиці пропускання існувала заради дотичного променя, тобто
// заради різкого краю горизонту. Тут різкого краю немає — усе вже усереднене по
// сфері напрямків.
//
// Рахується раз на набір параметрів повітря, як і пропускання: ні камери, ні
// напрямку на Сонце в осях немає (правило 5 етапу S).
//
// RGB — `ψ`, внесок другого й наступних порядків. **Альфа — найбільший із трьох
// каналів `f`**, частки, яку одне розсіювання повертає в ту саму точку. Живе
// вона там заради перевірки й не приховується: `ψ = L₂/(1 − f)` збігається
// лише при `f < 1`, і твердження «енергія не росте» перевіряється саме цим
// числом. Альфа в цій текстурі однаково є, тож коштує воно нуль.
[shader("compute")]
[numthreads(8, 8, 1)]
void multiscatter_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= MULTISCATTER_SIZE || id.y >= MULTISCATTER_SIZE) { return; }

    float bottom = air.shape.z;
    float top = air.shape.w;

    float u = float(id.x) / float(MULTISCATTER_SIZE - 1u);
    float v = float(id.y) / float(MULTISCATTER_SIZE - 1u);
    float mu_s = clamp(u * 2.0 - 1.0, -1.0, 1.0);
    float altitude = v * (top - bottom);
    float r = bottom + altitude;

    // Сонце в площині xz, точка на осі z.
    float3 sun = float3(sqrt(max(0.0, 1.0 - mu_s * mu_s)), 0.0, mu_s);

    // `rho²` початкової точки — **через висоту**, а не через різницю квадратів
    // радіусів: на рівні поверхні друге дорівнює нулю, і в `f32` йому дає знак
    // округлення. Промінь униз тоді не зупиняється на землі, а йде крізь
    // планету — і рядок 0 таблиці розходився з двійником удвічі, тоді як решта
    // сходилася на 0.05%. Те саме — `shell2` добутком, не різницею.
    float rho2 = altitude * (2.0 * bottom + altitude);
    float shell2 = (top - bottom) * (top + bottom);

    float3 second = float3(0.0, 0.0, 0.0);
    float3 fraction = float3(0.0, 0.0, 0.0);

    for (uint k = 0u; k < MULTISCATTER_DIRECTIONS; ++k)
    {
        float3 w = sphere_direction(k);
        float mu = w.z;

        float span = distance_to_top(r, mu, rho2, shell2);
        float ground = distance_to_ground(r, mu, rho2);
        if (ground >= 0.0) { span = min(span, ground); }
        float step = span / float(MULTISCATTER_STEPS);

        float3 throughput = float3(1.0, 1.0, 1.0);
        for (uint s = 0u; s < MULTISCATTER_STEPS; ++s)
        {
            float t = (float(s) + 0.5) * step;
            float3 point = float3(t * w.x, t * w.y, r + t * w.z);
            // `rho²` семпла — теж без різниці квадратів:
            // `|p + t·w|² − bottom² = rho² + 2·t·r·mu + t²`.
            float rho2_here = max(0.0, rho2 + 2.0 * t * r * mu + t * t);
            float radius = sqrt(max(0.0, rho2_here + bottom * bottom));
            // Висота з того самого `rho²`: сума великих чисел скорочення не має.
            float h = rho2_here / (radius + bottom);
            float mu_s_here = dot(point, sun) / max(radius, 1.0);

            // Тінь планети: під горизонтом точки прямого світла немає взагалі.
            // Звідси й нічний бік — його ніхто не малює окремо.
            bool lit = distance_to_ground(radius, mu_s_here, rho2_here) < 0.0;
            float3 to_sun = lit ? sample_transmittance(radius, mu_s_here)
                                : float3(0.0, 0.0, 0.0);

            float3 sigma_s = scattering(h);
            float3 sigma_e = extinction(h);

            for (uint c = 0u; c < 3u; ++c)
            {
                if (sigma_e[c] <= 0.0) { continue; }
                float step_transmittance = exp(-sigma_e[c] * step);
                float gain = (1.0 - step_transmittance) / sigma_e[c];

                // Фазова функція рівномірна — це і є наближення статті.
                float uniform_phase = 1.0 / (4.0 * 3.14159265358979);
                second[c] += throughput[c] * sigma_s[c] * to_sun[c] * uniform_phase * gain;
                // Частка: середовище світиться одиницею з усіх боків, тож
                // джерело — просто `σ_s`.
                fraction[c] += throughput[c] * sigma_s[c] * gain;

                throughput[c] *= step_transmittance;
            }
        }
    }

    // Середнє по сфері: `4π/N` тілесного кута на напрямок, поділене на `4π`
    // самого усереднення. Лишається `1/N`.
    second /= float(MULTISCATTER_DIRECTIONS);
    fraction /= float(MULTISCATTER_DIRECTIONS);

    float3 psi = second / max(float3(1.0, 1.0, 1.0) - fraction, float3(1.0e-6));
    multiscatter_out.Store(id.xy, float4(psi, max(fraction.r, max(fraction.g, fraction.b))));
}

// ---------------------------------------------------------------------------
// Небо (S4)
// ---------------------------------------------------------------------------

// Багаторазове розсіювання в точці `(r, mu_s)` — з таблиці, білінійно.
float3 sample_multiscatter(float r, float mu_s)
{
    float bottom = air.shape.z;
    float top = air.shape.w;
    float u = clamp(mu_s * 0.5 + 0.5, 0.0, 1.0);
    float v = clamp((r - bottom) / (top - bottom), 0.0, 1.0);
    float2 coord = float2(unit_to_texture(u, MULTISCATTER_SIZE),
                          unit_to_texture(v, MULTISCATTER_SIZE));
    return multiscatter_lut.SampleLevel(lut_sampler, coord, 0.0).rgb;
}

// Напрямок погляду за одиничними координатами таблиці неба — двійник
// `atmosphere::skyview_uv`. Повертає `(mu_v, cos_azimuth)`.
float2 skyview_uv(float r, float2 uv)
{
    float bottom = air.shape.z;
    float rho2 = max(0.0, r * r - bottom * bottom);
    float beta = acos(clamp(sqrt(rho2) / r, -1.0, 1.0));
    float zenith_horizon = PI - beta;

    float zenith;
    if (uv.y < 0.5)
    {
        float c = 1.0 - 2.0 * uv.y;
        zenith = zenith_horizon * (1.0 - c * c);
    }
    else
    {
        float c = 2.0 * uv.y - 1.0;
        zenith = zenith_horizon + beta * c * c;
    }
    return float2(cos(zenith), 1.0 - 2.0 * uv.x * uv.x);
}

// Фазова функція Релея — двійник `atmosphere::rayleigh_phase`.
float rayleigh_phase(float cos_theta)
{
    return 3.0 / (16.0 * PI) * (1.0 + cos_theta * cos_theta);
}

// Фазова функція Мі (Хеньї-Ґрінстайн) — двійник `atmosphere::mie_phase`.
float mie_phase(float cos_theta, float g)
{
    float denominator = 1.0 + g * g - 2.0 * g * cos_theta;
    return (1.0 - g * g) / (4.0 * PI * pow(max(denominator, 1.0e-6), 1.5));
}

// Розсіяне світло вздовж променя — двійник `atmosphere::sky_view`.
//
// Система координат локальна: `up = (0, 0, 1)`, Сонце в площині `xz`. Промінь
// зупиняється на поверхні й нічого до неї не додає — поверхню малює кадр, а не
// таблиця неба.
float3 raymarch_sky(float r, float mu_s, float mu_v, float cos_azimuth, uint steps)
{
    float bottom = air.shape.z;
    float top = air.shape.w;

    float3 sun = float3(sqrt(max(0.0, 1.0 - mu_s * mu_s)), 0.0, mu_s);
    float sin_v = sqrt(max(0.0, 1.0 - mu_v * mu_v));
    float sin_azimuth = sqrt(max(0.0, 1.0 - cos_azimuth * cos_azimuth));
    float3 w = float3(sin_v * cos_azimuth, sin_v * sin_azimuth, mu_v);

    // Кут розсіювання сталий уздовж променя: обидва напрямки нерухомі.
    float cos_theta = dot(w, sun);
    float phase_r = rayleigh_phase(cos_theta);
    float phase_m = mie_phase(cos_theta, air.mie.w);

    float rho2 = max(0.0, r * r - bottom * bottom);
    float shell2 = (top - bottom) * (top + bottom);
    float span = distance_to_top(r, mu_v, rho2, shell2);
    float ground = distance_to_ground(r, mu_v, rho2);
    if (ground >= 0.0) { span = min(span, ground); }
    float step = span / float(steps);

    float3 throughput = float3(1.0, 1.0, 1.0);
    float3 light = float3(0.0, 0.0, 0.0);
    for (uint s = 0u; s < steps; ++s)
    {
        float t = (float(s) + 0.5) * step;
        float3 point = float3(t * w.x, t * w.y, r + t * w.z);
        float rho2_here = max(0.0, rho2 + 2.0 * t * r * mu_v + t * t);
        float radius = sqrt(max(0.0, rho2_here + bottom * bottom));
        float h = rho2_here / (radius + bottom);
        float mu_s_here = dot(point, sun) / max(radius, 1.0);

        bool lit = distance_to_ground(radius, mu_s_here, rho2_here) < 0.0;
        float3 to_sun = lit ? sample_transmittance(radius, mu_s_here)
                            : float3(0.0, 0.0, 0.0);
        float3 psi = sample_multiscatter(radius, mu_s_here);

        float3 d = density(h);
        float3 sigma_e = extinction(h);
        float3 sigma_r = air.rayleigh.rgb * d.x;
        float sigma_m = air.mie.x * d.y;

        for (uint c = 0u; c < 3u; ++c)
        {
            if (sigma_e[c] <= 0.0) { continue; }
            // Пряме світло — з власною фазовою функцією кожної компоненти;
            // багаторазове вже усереднене по сфері, тож фази не має.
            float source = (sigma_r[c] * phase_r + sigma_m * phase_m) * to_sun[c]
                         + (sigma_r[c] + sigma_m) * psi[c];
            float step_transmittance = exp(-sigma_e[c] * step);
            light[c] += throughput[c] * source * (1.0 - step_transmittance) / sigma_e[c];
            throughput[c] *= step_transmittance;
        }
    }
    return light;
}

// Таблиця неба: 192×108, напрямок погляду → розсіяне світло.
//
// Рахується **раз на кадр**, бо залежить від позиції камери й напрямку на
// Сонце (правило 5 етапу S). Ціна не залежить ні від кількості апаратів, ні
// від того, що ще є в сцені: камера одна.
[shader("compute")]
[numthreads(8, 8, 1)]
void skyview_main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= SKYVIEW_WIDTH || id.y >= SKYVIEW_HEIGHT) { return; }

    float r = frame.view.x;
    float mu_s = frame.view.y;

    float2 uv = float2(float(id.x) / float(SKYVIEW_WIDTH - 1u),
                       float(id.y) / float(SKYVIEW_HEIGHT - 1u));
    float2 angles = skyview_uv(r, uv);

    float3 light = raymarch_sky(r, mu_s, angles.x, angles.y, SKYVIEW_STEPS);
    skyview_out.Store(id.xy, float4(light, 1.0));
}
