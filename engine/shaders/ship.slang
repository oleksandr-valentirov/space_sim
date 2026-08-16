// Корабель у кадрі (етап V, крок V2).
//
// Позиції у вершинному буфері вже camera-relative — та сама угода, що в
// line.slang і sphere.slang: CPU відняв позицію камери в double й звузив
// результат (ROADMAP F4, engine::camera). Тут лишається тільки проєкція.
//
// ⚠ **Усе тут — у КАМЕРНИХ осях**: позиції, нормалі й напрямок на світило.
// Позиції такими приходять за побудовою (`Camera::relative` повертає, а не
// лише віднімає), а з T5c шейдер бере з них напрямок на око — `view =
// −position`. Отже нормаль і світило мусять бути в тому самому просторі, і
// повертає їх той самий CPU-прохід. Робити це в шейдері означало б другу
// матрицю на корабель, тобто ще один uniform із динамічним зсувом — при тому,
// що вершин тут півтори тисячі, а не мільйон.
//
// Освітлення — Cook-Torrance з GGX (T5c), дослівний двійник `engine::brdf`.
// Правити треба обидва; оракул — збіг чисел, і його дає `tests/hull.rs`.
//
// Чого тут ще немає з моделі PROJECT.md §7: сяйва планети SH-пробою (T6) і
// напрямку на світило з ефемериди (борг D16). Ambient лишається нулем —
// вигаданого «трохи світла звідусіль» тут не буде.

// Колір сюди не входить: він атрибут вершини, з тієї самої причини, що в
// line.slang. Кораблів у кадрі кілька, виклик малювання на кожен, а всі записи
// в чергу відбуваються ДО проходу — тобто з uniform виграв би останній.
struct Uniforms
{
    float4x4 projection;
    float4 light_dir;   // напрямок ДО світила, КАМЕРНІ осі, xyz
};

// Динамічний зсув вибирає прохід глибини — так само, як у ламаних. Буфер
// один, крок 256 байтів (frame::PASS_STRIDE).
ConstantBuffer<Uniforms> uniforms;

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float4 colour : COLOR;
    // x — шорсткість, y — метал. Атрибут вершини, як і колір, і з тієї самої
    // причини: кораблів у кадрі кілька, а з uniform виграв би останній.
    float2 material : MATERIAL;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float3 normal : NORMAL;
    float4 colour : COLOR;
    nointerpolation float2 material : MATERIAL;
    // Позиція вершини **відносно камери** — з неї фрагмент бере напрямок на
    // око. Camera-relative тут не оптимізація, а сама угода буфера: позиція
    // вже прийшла такою, тож око стоїть у нулі, і напрямок до нього — це
    // просто `−position`.
    float3 view : VIEWDIR;
};

[shader("vertex")]
VertexOutput vertex_main(VertexInput input)
{
    VertexOutput output;
    output.position = mul(uniforms.projection, float4(input.position, 1.0));
    output.normal = input.normal;
    output.colour = input.colour;
    output.material = input.material;
    output.view = -input.position;
    return output;
}

// Нормальне відбиття діелектрика — `engine::brdf::DIELECTRIC_F0`.
static const float DIELECTRIC_F0 = 0.04;
// Найменша шорсткість — `engine::brdf::MIN_ROUGHNESS`. Нижче за неї пік `D`
// вибиває `f32`: нескінченність в одному пікселі й нуль у сусідньому.
static const float MIN_ROUGHNESS = 0.045;
static const float PI = 3.14159265358979323846;

// GGX / Trowbridge-Reitz — двійник `brdf::distribution`.
float distribution(float n_dot_h, float roughness)
{
    float a = max(roughness, MIN_ROUGHNESS);
    a = a * a;
    float a2 = a * a;
    float d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// Сміт з кореляцією висот, уже поділений на `4(n·l)(n·v)` — двійник
// `brdf::visibility`. Окремий знаменник давав би нуль на нуль на дотичних
// кутах; це той самий клас, що `max(x, 1e-30)` у повітрі.
float visibility(float n_dot_v, float n_dot_l, float roughness)
{
    float a = max(roughness, MIN_ROUGHNESS);
    a = a * a;
    float a2 = a * a;
    float v = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    float l = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(v + l, 1e-30);
}

// Шлік — двійник `brdf::fresnel`.
float3 fresnel(float3 f0, float v_dot_h)
{
    float t = saturate(1.0 - v_dot_h);
    float t5 = t * t * t * t * t;
    return f0 + (float3(1.0) - f0) * t5;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    float3 n = normalize(input.normal);
    float3 v = normalize(input.view);
    float3 l = uniforms.light_dir.xyz;

    // ⚠ Нормаль розвертається до ока, і це не косметика: пайплайн корабля
    // малює **без відсікання граней**, тож у кадр законно потрапляє
    // внутрішній бік оболонки. Без розвороту BRDF відповідає на такий
    // фрагмент нулем — і корпус дістає чорні плями там, де мусить бути
    // освітлена поверхня. Те саме робить `patch.slang` з нормаллю фасетки.
    if (dot(n, v) < 0.0)
    {
        n = -n;
    }

    float n_dot_l = dot(n, l);
    float n_dot_v = dot(n, v);
    if (n_dot_l <= 0.0)
    {
        return float4(0.0, 0.0, 0.0, 1.0);
    }

    float3 h = normalize(v + l);
    float n_dot_h = saturate(dot(n, h));
    float v_dot_h = saturate(dot(v, h));

    float roughness = input.material.x;
    float metallic = input.material.y;
    float3 base = input.colour.rgb;

    // Метал не має дифузного відбиття, а його `F0` — це базовий колір.
    float3 f0 = lerp(float3(DIELECTRIC_F0), base, metallic);
    float3 f = fresnel(f0, v_dot_h);
    float3 specular = distribution(n_dot_h, roughness)
        * visibility(n_dot_v, n_dot_l, roughness) * f;
    float3 diffuse = (float3(1.0) - f) * (1.0 - metallic) * base / PI;

    return float4((diffuse + specular) * n_dot_l, 1.0);
}
