struct AirParams_std140_0
{
    @align(16) rayleigh_0 : vec4<f32>,
    @align(16) mie_0 : vec4<f32>,
    @align(16) ozone_0 : vec4<f32>,
    @align(16) shape_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> air_0 : AirParams_std140_0;
@binding(1) @group(0) var transmittance_out_0 : texture_storage_2d<rgba16float, write>;

fn uv_to_r_mu_0( uv_0 : vec2<f32>,  r_0 : ptr<function, f32>,  mu_0 : ptr<function, f32>,  d_0 : ptr<function, f32>)
{
    var bottom_0 : f32 = air_0.shape_0.z;
    var top_0 : f32 = air_0.shape_0.w;
    var _S1 : f32 = bottom_0 * bottom_0;
    var h_0 : f32 = sqrt(max(0.0f, top_0 * top_0 - _S1));
    var rho_0 : f32 = h_0 * uv_0.y;
    var _S2 : f32 = rho_0 * rho_0;
    var _S3 : f32 = sqrt(_S2 + _S1);
    (*r_0) = _S3;
    var d_min_0 : f32 = top_0 - _S3;
    var _S4 : f32 = d_min_0 + uv_0.x * (rho_0 + h_0 - d_min_0);
    (*d_0) = _S4;
    var _S5 : f32;
    if(_S4 == 0.0f)
    {
        _S5 = 1.0f;
    }
    else
    {
        _S5 = clamp((h_0 * h_0 - _S2 - (*d_0) * (*d_0)) / (2.0f * (*r_0) * (*d_0)), -1.0f, 1.0f);
    }
    (*mu_0) = _S5;
    return;
}

fn density_0( h_1 : f32) -> vec3<f32>
{
    var _S6 : f32 = - h_1;
    return vec3<f32>(exp(_S6 / air_0.rayleigh_0.w), exp(_S6 / air_0.mie_0.z), max(0.0f, 1.0f - abs(h_1 - air_0.shape_0.x) / air_0.shape_0.y));
}

fn extinction_0( h_2 : f32) -> vec3<f32>
{
    var d_1 : vec3<f32> = density_0(h_2);
    var mie_1 : f32 = air_0.mie_0.x + air_0.mie_0.y;
    return air_0.rayleigh_0.xyz * vec3<f32>(d_1.x) + vec3<f32>(mie_1, mie_1, mie_1) * vec3<f32>(d_1.y) + air_0.ozone_0.xyz * vec3<f32>(d_1.z);
}

fn optical_depth_0( r_1 : f32,  mu_1 : f32,  span_0 : f32,  steps_0 : u32) -> vec3<f32>
{
    var _S7 : f32 = air_0.shape_0.z;
    var _S8 : f32 = span_0 / f32(steps_0);
    const _S9 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var k_0 : u32 = u32(0);
    var sum_0 : vec3<f32> = _S9;
    for(;;)
    {
        if(k_0 < steps_0)
        {
        }
        else
        {
            break;
        }
        var d_2 : f32 = (f32(k_0) + 0.5f) * _S8;
        var sum_1 : vec3<f32> = sum_0 + extinction_0(sqrt(max(0.0f, r_1 * r_1 + d_2 * d_2 + 2.0f * r_1 * d_2 * mu_1)) - _S7) * vec3<f32>(_S8);
        k_0 = k_0 + u32(1);
        sum_0 = sum_1;
    }
    return sum_0;
}

@compute
@workgroup_size(8, 8, 1)
fn transmittance_main(@builtin(global_invocation_id) id_0 : vec3<u32>)
{
    var _S10 : u32 = id_0.x;
    var _S11 : bool;
    if(_S10 >= u32(256))
    {
        _S11 = true;
    }
    else
    {
        _S11 = (id_0.y) >= u32(64);
    }
    if(_S11)
    {
        return;
    }
    var r_2 : f32;
    var mu_2 : f32;
    var span_1 : f32;
    uv_to_r_mu_0(vec2<f32>(f32(_S10) / 255.0f, f32(id_0.y) / 63.0f), &(r_2), &(mu_2), &(span_1));
    textureStore((transmittance_out_0), (id_0.xy), (vec4<f32>(exp((vec3<f32>(0) - optical_depth_0(r_2, mu_2, span_1, u32(500)))), 1.0f)));
    return;
}

