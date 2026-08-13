// Той самий трикутник, але вершини приходять буфером, а не рахуються з
// SV_VertexID. Існує заради одного питання розвідки P1.
//
// triangle.slang не завантажується як SPIR-V: naga відхиляє capability
// DrawParameters, яку Slang оголошує заради SV_VertexID. Питання, на яке
// відповідає цей файл, — чи ламається SPIR-V узагалі, чи лише ця конструкція.
// Відповідь змінює висновок: «шлях непридатний» і «шлях придатний, крім
// одного дуже поширеного прийому» — різні речі.
//
// Малювати ним зонд не намагається: для цього потрібні вершинні буфери й
// розкладка, а питання тут не про малювання, а про те, що приймає naga.

struct VertexOutput
{
    float4 position : SV_Position;
    float3 color : COLOR;
};

[shader("vertex")]
VertexOutput vertex_main(float2 position: POSITION, float3 color: COLOR)
{
    VertexOutput output;
    output.position = float4(position, 0.0, 1.0);
    output.color = color;
    return output;
}

[shader("fragment")]
float4 fragment_main(VertexOutput input) : SV_Target
{
    return float4(input.color, 1.0);
}
