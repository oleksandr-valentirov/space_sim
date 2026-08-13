// Траєкторія: перетворення фрейму у вертексному шейдері (ROADMAP F6).
//
// Вершинні дані — геоцентричні: vessel_local і moon_local уже без Землі
// (Земля в кожній вершині своя, для миті цього семпла), а не через
// відняття камери. Це той самий camera-relative принцип (F4, F5), лише
// прив'язаний до Землі-в-момент-t замість камери: апарат ніколи не
// відходить далі кількох Мм від Місяця, тож числа лишаються малі на всій
// 233-денній місії, хоч сама Земля тим часом проходить по геліоцентричній
// орбіті мільярди метрів.
//
// Обертовий фрейм рахується тут, а не на CPU: та сама формула, що вже
// звірена з `frame_from_inertial` (C, `core/frame.h`) у
// `engine::trajectory::rotating_position` — `engine/tests/trajectory.rs`
// звіряє їх до 7·10⁻⁷ дистанційних одиниць.
//
// Два вершинних входи, не uniform-прапорець. Спершу було зроблено через
// `mix(geocentric, rotating, uniforms.rotating)` в одній точці входу — і
// це стабільно давало порожній кадр саме для rotating=1 на цьому GPU
// (AMD RADV VANGOGH, ACO): фрагментний шейдер читав uniform правильно
// (перевірено виводом значення в колір), а вершинний — ні, навіть через
// проміжну локальну змінну. Причина лишилась незʼясованою — findings такого
// роду йдуть у ROADMAP, не в код; тут висновок практичний: дві точки входу
// однозначно уникають того самого шляху компілятора, і «перемикання
// миттєве» від цього не постраждало — обидва пайплайни компілюються один
// раз при першому виклику `render`, вершинні буфери спільні, перемикання
// фрейму — це вибір готового пайплайна, не перерахунок вершин.

struct Uniforms
{
    float4x4 projection;
    float mu;
    float _pad0;
    float2 _pad1;
    float3 view_offset;
    float _pad2;
    float4 colour;
};

ConstantBuffer<Uniforms> uniforms;

struct VertexInput
{
    float3 vessel_local : POSITION;   // апарат мінус Земля в момент t
    float3 moon_local : TEXCOORD0;    // Місяць мінус Земля в момент t (= d)
    float3 z_axis : NORMAL;           // нормаль миттєвої площини Земля-Місяць
};

struct VertexOutput
{
    float4 position : SV_Position;
};

/// Спільна частина обох точок входу: ортонормований базис (x, y, z) з
/// Земля-Місяць лінії та миттєвої нормалі площини.
float3x3 synodic_basis(float3 moon_local, float3 z_axis, out float length_scale)
{
    float3 d = moon_local;
    length_scale = length(d);
    float3 x_axis = d / length_scale;
    float3 y_axis = cross(z_axis, x_axis);
    return float3x3(x_axis, y_axis, z_axis);
}

float4 project(float3 view_basis_pos, Uniforms u)
{
    // Камера дивиться вздовж «y» цього базису: розкид орбіти там на порядки
    // менший, ніж по x (уздовж Земля-Місяць) і z (упоперек площини) —
    // виміряно (ROADMAP F6), тож саме під цим кутом видно форму, а не лінію.
    float3 view = float3(view_basis_pos.x, view_basis_pos.z, -view_basis_pos.y) + u.view_offset;
    return mul(u.projection, float4(view, 1.0));
}

[shader("vertex")]
VertexOutput vertex_geocentric(VertexInput input)
{
    float length_scale;
    float3x3 basis = synodic_basis(input.moon_local, input.z_axis, length_scale);

    // Щирий інерціальний вигляд: той самий базис для кута огляду, але без
    // зсуву початку координат і без масштабування.
    float3 geocentric = mul(basis, input.vessel_local);

    VertexOutput output;
    output.position = project(geocentric, uniforms);
    return output;
}

[shader("vertex")]
VertexOutput vertex_rotating(VertexInput input)
{
    float length_scale;
    float3x3 basis = synodic_basis(input.moon_local, input.z_axis, length_scale);

    // Початок координат — барицентр (mu-зважений), масштаб — L. Формула —
    // дзеркало engine::trajectory::rotating_position.
    float3 origin = uniforms.mu * input.moon_local;
    float3 rotating = mul(basis, input.vessel_local - origin) / length_scale;

    VertexOutput output;
    output.position = project(rotating, uniforms);
    return output;
}

[shader("fragment")]
float4 fragment_main() : SV_Target
{
    return uniforms.colour;
}
