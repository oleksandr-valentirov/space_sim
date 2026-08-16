struct AirParams_std140_0
{
    @align(16) rayleigh_0 : vec4<f32>,
    @align(16) mie_0 : vec4<f32>,
    @align(16) ozone_0 : vec4<f32>,
    @align(16) shape_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> air_0 : AirParams_std140_0;
@binding(0) @group(1) var transmittance_out_0 : texture_storage_2d<rgba16float, write>;

@binding(2) @group(0) var transmittance_lut_0 : texture_2d<f32>;

@binding(1) @group(0) var lut_sampler_0 : sampler;

@binding(1) @group(1) var multiscatter_out_0 : texture_storage_2d<rgba16float, write>;

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
    var _S6 : f32 = max(h_1, 0.0f);
    var _S7 : f32 = - _S6;
    return vec3<f32>(exp(_S7 / air_0.rayleigh_0.w), exp(_S7 / air_0.mie_0.z), max(0.0f, 1.0f - abs(_S6 - air_0.shape_0.x) / air_0.shape_0.y));
}

fn extinction_0( h_2 : f32) -> vec3<f32>
{
    var d_1 : vec3<f32> = density_0(h_2);
    var mie_1 : f32 = air_0.mie_0.x + air_0.mie_0.y;
    return air_0.rayleigh_0.xyz * vec3<f32>(d_1.x) + vec3<f32>(mie_1, mie_1, mie_1) * vec3<f32>(d_1.y) + air_0.ozone_0.xyz * vec3<f32>(d_1.z);
}

fn optical_depth_0( r_1 : f32,  mu_1 : f32,  span_0 : f32,  steps_0 : u32) -> vec3<f32>
{
    var _S8 : f32 = air_0.shape_0.z;
    var _S9 : f32 = span_0 / f32(steps_0);
    const _S10 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var k_0 : u32 = u32(0);
    var sum_0 : vec3<f32> = _S10;
    for(;;)
    {
        if(k_0 < steps_0)
        {
        }
        else
        {
            break;
        }
        var d_2 : f32 = (f32(k_0) + 0.5f) * _S9;
        var sum_1 : vec3<f32> = sum_0 + extinction_0(sqrt(max(0.0f, r_1 * r_1 + d_2 * d_2 + 2.0f * r_1 * d_2 * mu_1)) - _S8) * vec3<f32>(_S9);
        k_0 = k_0 + u32(1);
        sum_0 = sum_1;
    }
    return sum_0;
}

@compute
@workgroup_size(8, 8, 1)
fn transmittance_main(@builtin(global_invocation_id) id_0 : vec3<u32>)
{
    var _S11 : u32 = id_0.x;
    var _S12 : bool;
    if(_S11 >= u32(256))
    {
        _S12 = true;
    }
    else
    {
        _S12 = (id_0.y) >= u32(64);
    }
    if(_S12)
    {
        return;
    }
    var r_2 : f32;
    var mu_2 : f32;
    var span_1 : f32;
    uv_to_r_mu_0(vec2<f32>(f32(_S11) / 255.0f, f32(id_0.y) / 63.0f), &(r_2), &(mu_2), &(span_1));
    textureStore((transmittance_out_0), (id_0.xy), (vec4<f32>(exp((vec3<f32>(0) - optical_depth_0(r_2, mu_2, span_1, u32(500)))), 1.0f)));
    return;
}

fn sphere_direction_0( k_1 : u32) -> vec3<f32>
{
    var theta_0 : f32 = 6.28318548202514648f * (0.5f + f32(k_1 / u32(8))) / 8.0f;
    var phi_0 : f32 = acos(1.0f - 2.0f * (0.5f + f32(k_1 % u32(8))) / 8.0f);
    var _S13 : f32 = sin(phi_0);
    return vec3<f32>(_S13 * cos(theta_0), _S13 * sin(theta_0), cos(phi_0));
}

fn distance_to_top_0( r_3 : f32,  mu_3 : f32,  rho2_0 : f32,  shell2_0 : f32) -> f32
{
    return max(0.0f, - r_3 * mu_3 + sqrt(max(0.0f, r_3 * r_3 * mu_3 * mu_3 + (shell2_0 - rho2_0))));
}

fn distance_to_ground_0( r_4 : f32,  mu_4 : f32,  rho2_1 : f32) -> f32
{
    var discriminant_0 : f32 = r_4 * r_4 * mu_4 * mu_4 - rho2_1;
    var _S14 : bool;
    if(mu_4 >= 0.0f)
    {
        _S14 = true;
    }
    else
    {
        _S14 = discriminant_0 < 0.0f;
    }
    if(_S14)
    {
        return -1.0f;
    }
    return max(0.0f, - r_4 * mu_4 - sqrt(discriminant_0));
}

fn r_mu_to_uv_0( r_5 : f32,  mu_5 : f32) -> vec2<f32>
{
    var bottom_1 : f32 = air_0.shape_0.z;
    var top_1 : f32 = air_0.shape_0.w;
    var _S15 : f32 = bottom_1 * bottom_1;
    var h_3 : f32 = sqrt(max(0.0f, top_1 * top_1 - _S15));
    var rho_1 : f32 = sqrt(max(0.0f, r_5 * r_5 - _S15));
    var d_3 : f32 = distance_to_top_0(r_5, mu_5, rho_1 * rho_1, h_3 * h_3);
    var d_min_1 : f32 = top_1 - r_5;
    var d_max_0 : f32 = rho_1 + h_3;
    var u_0 : f32;
    if(d_max_0 > d_min_1)
    {
        u_0 = clamp((d_3 - d_min_1) / (d_max_0 - d_min_1), 0.0f, 1.0f);
    }
    else
    {
        u_0 = 0.0f;
    }
    return vec2<f32>(u_0, clamp(rho_1 / h_3, 0.0f, 1.0f));
}

fn unit_to_texture_0( u_1 : f32,  size_0 : u32) -> f32
{
    var n_0 : f32 = f32(size_0);
    return 0.5f / n_0 + u_1 * (1.0f - 1.0f / n_0);
}

fn sample_transmittance_0( r_6 : f32,  mu_6 : f32) -> vec3<f32>
{
    var uv_1 : vec2<f32> = r_mu_to_uv_0(r_6, mu_6);
    return (textureSampleLevel((transmittance_lut_0), (lut_sampler_0), (vec2<f32>(unit_to_texture_0(uv_1.x, u32(256)), unit_to_texture_0(uv_1.y, u32(64)))), (0.0f))).xyz;
}

fn scattering_0( h_4 : f32) -> vec3<f32>
{
    var d_4 : vec3<f32> = density_0(h_4);
    var mie_2 : f32 = air_0.mie_0.x * d_4.y;
    return air_0.rayleigh_0.xyz * vec3<f32>(d_4.x) + vec3<f32>(mie_2, mie_2, mie_2);
}

@compute
@workgroup_size(8, 8, 1)
fn multiscatter_main(@builtin(global_invocation_id) id_1 : vec3<u32>)
{
    var _S16 : u32 = id_1.x;
    var _S17 : bool;
    if(_S16 >= u32(32))
    {
        _S17 = true;
    }
    else
    {
        _S17 = (id_1.y) >= u32(32);
    }
    if(_S17)
    {
        return;
    }
    var bottom_2 : f32 = air_0.shape_0.z;
    var top_2 : f32 = air_0.shape_0.w;
    var mu_s_0 : f32 = clamp(f32(_S16) / 31.0f * 2.0f - 1.0f, -1.0f, 1.0f);
    var _S18 : f32 = top_2 - bottom_2;
    var altitude_0 : f32 = f32(id_1.y) / 31.0f * _S18;
    var _S19 : f32 = bottom_2 + altitude_0;
    var _S20 : vec3<f32> = vec3<f32>(sqrt(max(0.0f, 1.0f - mu_s_0 * mu_s_0)), 0.0f, mu_s_0);
    var _S21 : f32 = altitude_0 * (2.0f * bottom_2 + altitude_0);
    var _S22 : f32 = _S18 * (top_2 + bottom_2);
    const _S23 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var second_0 : vec3<f32> = _S23;
    var fraction_0 : vec3<f32> = _S23;
    var k_2 : u32 = u32(0);
    for(;;)
    {
        if(k_2 < u32(64))
        {
        }
        else
        {
            break;
        }
        var w_0 : vec3<f32> = sphere_direction_0(k_2);
        var mu_7 : f32 = w_0.z;
        var span_2 : f32 = distance_to_top_0(_S19, mu_7, _S21, _S22);
        var ground_0 : f32 = distance_to_ground_0(_S19, mu_7, _S21);
        var span_3 : f32;
        if(ground_0 >= 0.0f)
        {
            span_3 = min(span_2, ground_0);
        }
        else
        {
            span_3 = span_2;
        }
        var _S24 : f32 = span_3 / 20.0f;
        var throughput_0 : vec3<f32> = vec3<f32>(1.0f, 1.0f, 1.0f);
        var s_0 : u32 = u32(0);
        for(;;)
        {
            if(s_0 < u32(20))
            {
            }
            else
            {
                break;
            }
            var t_0 : f32 = (f32(s_0) + 0.5f) * _S24;
            var _S25 : f32 = max(0.0f, _S21 + 2.0f * t_0 * _S19 * mu_7 + t_0 * t_0);
            var radius_0 : f32 = sqrt(max(0.0f, _S25 + bottom_2 * bottom_2));
            var h_5 : f32 = _S25 / (radius_0 + bottom_2);
            var mu_s_here_0 : f32 = dot(vec3<f32>(t_0 * w_0.x, t_0 * w_0.y, _S19 + t_0 * mu_7), _S20) / max(radius_0, 1.0f);
            var _S26 : vec3<f32>;
            if((distance_to_ground_0(radius_0, mu_s_here_0, _S25)) < 0.0f)
            {
                _S26 = sample_transmittance_0(radius_0, mu_s_here_0);
            }
            else
            {
                _S26 = _S23;
            }
            var _S27 : vec3<f32> = scattering_0(h_5);
            var _S28 : vec3<f32> = extinction_0(h_5);
            var c_0 : u32 = u32(0);
            for(;;)
            {
                if(c_0 < u32(3))
                {
                }
                else
                {
                    break;
                }
                var _S29 : u32 = c_0;
                if((_S28[c_0]) <= 0.0f)
                {
                    c_0 = c_0 + u32(1);
                    continue;
                }
                var step_transmittance_0 : f32 = exp(- _S28[_S29] * _S24);
                var gain_0 : f32 = (1.0f - step_transmittance_0) / _S28[_S29];
                second_0[c_0] = second_0[c_0] + throughput_0[c_0] * _S27[c_0] * _S26[c_0] * 0.07957746833562851f * gain_0;
                fraction_0[c_0] = fraction_0[c_0] + throughput_0[c_0] * _S27[c_0] * gain_0;
                throughput_0[c_0] = throughput_0[c_0] * step_transmittance_0;
                c_0 = c_0 + u32(1);
            }
            s_0 = s_0 + u32(1);
        }
        k_2 = k_2 + u32(1);
    }
    var _S30 : vec3<f32> = vec3<f32>(64.0f);
    var _S31 : vec3<f32> = second_0 / _S30;
    second_0 = _S31;
    var _S32 : vec3<f32> = fraction_0 / _S30;
    fraction_0 = _S32;
    textureStore((multiscatter_out_0), (id_1.xy), (vec4<f32>(_S31 / max(vec3<f32>(1.0f, 1.0f, 1.0f) - _S32, vec3<f32>(9.99999997475242708e-07f)), max(_S32.x, max(_S32.y, _S32.z)))));
    return;
}

