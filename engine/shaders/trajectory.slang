// Trajectory: the frame transform in the vertex shader (ROADMAP F6).
//
// The vertex data is geocentric: vessel_local and moon_local already have
// Earth subtracted (a different Earth per vertex, for that sample's instant)
// rather than the camera. This is the same camera-relative principle (F4,
// F5), only tied to Earth-at-time-t instead of the camera: the vessel never
// goes further than a few Mm from the Moon, so the numbers stay small across
// the whole 233-day mission, while Earth itself travels billions of metres
// along its heliocentric orbit.
//
// The rotating frame is computed here rather than on the CPU: the same
// formula already compared with `frame_from_inertial` (C, `core/frame.h`) in
// `engine::trajectory::rotating_position` -- `engine/tests/trajectory.rs`
// matches them to 7e-7 distance units.
//
// Two vertex entry points, not a uniform flag. The first version used
// `mix(geocentric, rotating, uniforms.rotating)` in one entry point -- and it
// reliably gave an empty frame for rotating=1 on this GPU (AMD RADV VANGOGH,
// ACO): the fragment shader read the uniform correctly (verified by writing
// the value into the colour) while the vertex shader did not, even through an
// intermediate local. The cause was never established -- findings of that
// kind go into ROADMAP, not into code; the practical conclusion is here: two
// entry points unambiguously avoid that compiler path, and "switching is
// instant" did not suffer -- both pipelines are compiled once on the first
// `render` call, the vertex buffers are shared, and switching the frame picks
// a ready pipeline rather than recomputing vertices.

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
    float3 vessel_local : POSITION;   // vessel minus Earth at time t
    float3 moon_local : TEXCOORD0;    // Moon minus Earth at time t (= d)
    float3 z_axis : NORMAL;           // normal of the instantaneous Earth-Moon plane
};

struct VertexOutput
{
    float4 position : SV_Position;
};

/// The shared part of both entry points: an orthonormal basis (x, y, z) from
/// the Earth-Moon line and the instantaneous plane normal.
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
    // The camera looks along the "y" of this basis: the orbit's spread there
    // is orders smaller than along x (the Earth-Moon line) or z (across the
    // plane) -- measured (ROADMAP F6), so it is from this angle that the shape
    // is visible rather than a line.
    float3 view = float3(view_basis_pos.x, view_basis_pos.z, -view_basis_pos.y) + u.view_offset;
    return mul(u.projection, float4(view, 1.0));
}

[shader("vertex")]
VertexOutput vertex_geocentric(VertexInput input)
{
    float length_scale;
    float3x3 basis = synodic_basis(input.moon_local, input.z_axis, length_scale);

    // An honest inertial view: the same basis for the viewing angle, but with
    // no origin shift and no scaling.
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

    // The origin is the barycentre (mu-weighted), the scale is L. The formula
    // mirrors engine::trajectory::rotating_position.
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
