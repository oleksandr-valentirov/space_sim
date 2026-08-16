struct AirParams_std140_0
{
    @align(16) rayleigh_0 : vec4<f32>,
    @align(16) mie_0 : vec4<f32>,
    @align(16) ozone_0 : vec4<f32>,
    @align(16) shape_0 : vec4<f32>,
    @align(16) ground_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> air_0 : AirParams_std140_0;
@binding(0) @group(1) var transmittance_out_0 : texture_storage_2d<rgba16float, write>;

@binding(2) @group(0) var transmittance_lut_0 : texture_2d<f32>;

@binding(1) @group(0) var lut_sampler_0 : sampler;

@binding(1) @group(1) var multiscatter_out_0 : texture_storage_2d<rgba16float, write>;

struct ViewParams_std140_0
{
    @align(16) view_0 : vec4<f32>,
    @align(16) eye_0 : vec4<f32>,
    @align(16) sun_0 : vec4<f32>,
    @align(16) right_0 : vec4<f32>,
    @align(16) screen_up_0 : vec4<f32>,
    @align(16) forward_0 : vec4<f32>,
};

@binding(4) @group(0) var<uniform> frame_0 : ViewParams_std140_0;
@binding(3) @group(0) var multiscatter_lut_0 : texture_2d<f32>;

@binding(2) @group(1) var skyview_out_0 : texture_storage_2d<rgba16float, write>;

@binding(5) @group(0) var skyview_lut_0 : texture_2d<f32>;

@binding(3) @group(1) var aerial_inscatter_out_0 : texture_storage_3d<rgba16float, write>;

@binding(4) @group(1) var aerial_transmittance_out_0 : texture_storage_3d<rgba16float, write>;

@binding(8) @group(0) var depth_texture_0 : texture_2d<f32>;

struct PassParams_std140_0
{
    @align(16) depth_0 : vec4<f32>,
};

@binding(9) @group(0) var<uniform> range_0 : PassParams_std140_0;
@binding(7) @group(0) var aerial_transmittance_lut_0 : texture_3d<f32>;

@binding(6) @group(0) var aerial_inscatter_lut_0 : texture_3d<f32>;

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
    var to_sun_0 : vec3<f32>;
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
    var k_2 : u32 = u32(0);
    var second_0 : vec3<f32> = _S23;
    var fraction_0 : vec3<f32> = _S23;
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
        var ground_1 : f32 = distance_to_ground_0(_S19, mu_7, _S21);
        var _S24 : bool = ground_1 >= 0.0f;
        var span_3 : f32;
        if(_S24)
        {
            span_3 = min(span_2, ground_1);
        }
        else
        {
            span_3 = span_2;
        }
        var _S25 : f32 = span_3 / 20.0f;
        const _S26 : vec3<f32> = vec3<f32>(1.0f, 1.0f, 1.0f);
        var s_0 : u32 = u32(0);
        var throughput_0 : vec3<f32> = _S26;
        var second_1 : vec3<f32> = second_0;
        var fraction_1 : vec3<f32> = fraction_0;
        for(;;)
        {
            if(s_0 < u32(20))
            {
            }
            else
            {
                break;
            }
            var t_0 : f32 = (f32(s_0) + 0.5f) * _S25;
            var _S27 : f32 = max(0.0f, _S21 + 2.0f * t_0 * _S19 * mu_7 + t_0 * t_0);
            var radius_0 : f32 = sqrt(max(0.0f, _S27 + bottom_2 * bottom_2));
            var h_5 : f32 = _S27 / (radius_0 + bottom_2);
            var mu_s_here_0 : f32 = dot(vec3<f32>(t_0 * w_0.x, t_0 * w_0.y, _S19 + t_0 * mu_7), _S20) / max(radius_0, 1.0f);
            if((distance_to_ground_0(radius_0, mu_s_here_0, _S27)) < 0.0f)
            {
                to_sun_0 = sample_transmittance_0(radius_0, mu_s_here_0);
            }
            else
            {
                to_sun_0 = _S23;
            }
            var sigma_e_0 : vec3<f32> = extinction_0(h_5);
            var step_transmittance_0 : vec3<f32> = exp((vec3<f32>(0) - sigma_e_0) * vec3<f32>(_S25));
            var gain_0 : vec3<f32> = (vec3<f32>(1.0f) - step_transmittance_0) / max(sigma_e_0, vec3<f32>(1.00000000317107685e-30f));
            var _S28 : vec3<f32> = throughput_0 * scattering_0(h_5);
            var second_2 : vec3<f32> = second_1 + _S28 * to_sun_0 * vec3<f32>(0.07957746833562851f) * gain_0;
            var fraction_2 : vec3<f32> = fraction_1 + _S28 * gain_0;
            var throughput_1 : vec3<f32> = throughput_0 * step_transmittance_0;
            s_0 = s_0 + u32(1);
            throughput_0 = throughput_1;
            second_1 = second_2;
            fraction_1 = fraction_2;
        }
        if(_S24)
        {
            var mu_s_ground_0 : f32 = dot(normalize(vec3<f32>(ground_1 * w_0.x, ground_1 * w_0.y, _S19 + ground_1 * mu_7)), _S20);
            if(mu_s_ground_0 > 0.0f)
            {
                to_sun_0 = second_1 + throughput_0 * air_0.ground_0.xyz / vec3<f32>(3.14159274101257324f) * sample_transmittance_0(bottom_2, mu_s_ground_0) * vec3<f32>(mu_s_ground_0);
            }
            else
            {
                to_sun_0 = second_1;
            }
            var fraction_3 : vec3<f32> = fraction_1 + throughput_0 * air_0.ground_0.xyz;
            second_0 = to_sun_0;
            fraction_0 = fraction_3;
        }
        else
        {
            second_0 = second_1;
            fraction_0 = fraction_1;
        }
        k_2 = k_2 + u32(1);
    }
    var _S29 : vec3<f32> = vec3<f32>(64.0f);
    var fraction_4 : vec3<f32> = fraction_0 / _S29;
    textureStore((multiscatter_out_0), (id_1.xy), (vec4<f32>(second_0 / _S29 / max(vec3<f32>(1.0f, 1.0f, 1.0f) - fraction_4, vec3<f32>(9.99999997475242708e-07f)), max(fraction_4.x, max(fraction_4.y, fraction_4.z)))));
    return;
}

fn skyview_uv_0( r_7 : f32,  uv_2 : vec2<f32>) -> vec2<f32>
{
    var bottom_3 : f32 = air_0.shape_0.z;
    var beta_0 : f32 = acos(clamp(sqrt(max(0.0f, r_7 * r_7 - bottom_3 * bottom_3)) / r_7, -1.0f, 1.0f));
    var zenith_horizon_0 : f32 = 3.14159274101257324f - beta_0;
    var _S30 : f32 = uv_2.y;
    var zenith_0 : f32;
    if(_S30 < 0.5f)
    {
        var c_0 : f32 = 1.0f - 2.0f * _S30;
        zenith_0 = zenith_horizon_0 * (1.0f - c_0 * c_0);
    }
    else
    {
        var c_1 : f32 = 2.0f * _S30 - 1.0f;
        zenith_0 = zenith_horizon_0 + beta_0 * c_1 * c_1;
    }
    var _S31 : f32 = uv_2.x;
    return vec2<f32>(cos(zenith_0), 1.0f - 2.0f * _S31 * _S31);
}

fn rayleigh_phase_0( cos_theta_0 : f32) -> f32
{
    return 0.05968310311436653f * (1.0f + cos_theta_0 * cos_theta_0);
}

fn mie_phase_0( cos_theta_1 : f32,  g_0 : f32) -> f32
{
    var _S32 : f32 = g_0 * g_0;
    return (1.0f - _S32) / (12.56637096405029297f * pow(max(1.0f + _S32 - 2.0f * g_0 * cos_theta_1, 9.99999997475242708e-07f), 1.5f));
}

fn sample_multiscatter_0( r_8 : f32,  mu_s_1 : f32) -> vec3<f32>
{
    var bottom_4 : f32 = air_0.shape_0.z;
    return (textureSampleLevel((multiscatter_lut_0), (lut_sampler_0), (vec2<f32>(unit_to_texture_0(clamp(mu_s_1 * 0.5f + 0.5f, 0.0f, 1.0f), u32(32)), unit_to_texture_0(clamp((r_8 - bottom_4) / (air_0.shape_0.w - bottom_4), 0.0f, 1.0f), u32(32)))), (0.0f))).xyz;
}

fn raymarch_0( pos_0 : vec3<f32>,  dir_0 : vec3<f32>,  sun_1 : vec3<f32>,  rho2_2 : f32,  span_4 : f32,  steps_1 : u32) -> vec3<f32>
{
    var _S33 : f32 = air_0.shape_0.z;
    var r_9 : f32 = length(pos_0);
    var _S34 : f32 = dot(pos_0, dir_0) / max(r_9, 1.0f);
    var cos_theta_2 : f32 = dot(dir_0, sun_1);
    var _S35 : f32 = rayleigh_phase_0(cos_theta_2);
    var _S36 : f32 = mie_phase_0(cos_theta_2, air_0.mie_0.w);
    var _S37 : f32 = span_4 / f32(steps_1);
    const _S38 : vec3<f32> = vec3<f32>(1.0f, 1.0f, 1.0f);
    const _S39 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var s_1 : u32 = u32(0);
    var throughput_2 : vec3<f32> = _S38;
    var light_0 : vec3<f32> = _S39;
    for(;;)
    {
        if(s_1 < steps_1)
        {
        }
        else
        {
            break;
        }
        var t_1 : f32 = (f32(s_1) + 0.5f) * _S37;
        var _S40 : f32 = max(0.0f, rho2_2 + 2.0f * t_1 * r_9 * _S34 + t_1 * t_1);
        var radius_1 : f32 = sqrt(max(0.0f, _S40 + _S33 * _S33));
        var h_6 : f32 = _S40 / (radius_1 + _S33);
        var mu_s_here_1 : f32 = dot(pos_0 + vec3<f32>(t_1) * dir_0, sun_1) / max(radius_1, 1.0f);
        var to_sun_1 : vec3<f32>;
        if((distance_to_ground_0(radius_1, mu_s_here_1, _S40)) < 0.0f)
        {
            to_sun_1 = sample_transmittance_0(radius_1, mu_s_here_1);
        }
        else
        {
            to_sun_1 = _S39;
        }
        var d_5 : vec3<f32> = density_0(h_6);
        var sigma_e_1 : vec3<f32> = extinction_0(h_6);
        var sigma_r_0 : vec3<f32> = air_0.rayleigh_0.xyz * vec3<f32>(d_5.x);
        var sigma_m_0 : f32 = air_0.mie_0.x * d_5.y;
        var step_transmittance_1 : vec3<f32> = exp((vec3<f32>(0) - sigma_e_1) * vec3<f32>(_S37));
        var light_1 : vec3<f32> = light_0 + throughput_2 * ((sigma_r_0 * vec3<f32>(_S35) + vec3<f32>((sigma_m_0 * _S36))) * to_sun_1 + (sigma_r_0 + vec3<f32>(sigma_m_0)) * sample_multiscatter_0(radius_1, mu_s_here_1)) * (vec3<f32>(1.0f) - step_transmittance_1) / max(sigma_e_1, vec3<f32>(1.00000000317107685e-30f));
        var throughput_3 : vec3<f32> = throughput_2 * step_transmittance_1;
        s_1 = s_1 + u32(1);
        throughput_2 = throughput_3;
        light_0 = light_1;
    }
    return light_0;
}

fn raymarch_sky_0( r_10 : f32,  mu_s_2 : f32,  mu_v_0 : f32,  cos_azimuth_0 : f32,  steps_2 : u32) -> vec3<f32>
{
    var bottom_5 : f32 = air_0.shape_0.z;
    var top_3 : f32 = air_0.shape_0.w;
    var sun_2 : vec3<f32> = vec3<f32>(sqrt(max(0.0f, 1.0f - mu_s_2 * mu_s_2)), 0.0f, mu_s_2);
    var sin_v_0 : f32 = sqrt(max(0.0f, 1.0f - mu_v_0 * mu_v_0));
    var w_1 : vec3<f32> = vec3<f32>(sin_v_0 * cos_azimuth_0, sin_v_0 * sqrt(max(0.0f, 1.0f - cos_azimuth_0 * cos_azimuth_0)), mu_v_0);
    var _S41 : f32 = max(0.0f, r_10 * r_10 - bottom_5 * bottom_5);
    var span_5 : f32 = distance_to_top_0(r_10, mu_v_0, _S41, (top_3 - bottom_5) * (top_3 + bottom_5));
    var ground_2 : f32 = distance_to_ground_0(r_10, mu_v_0, _S41);
    var span_6 : f32;
    if(ground_2 >= 0.0f)
    {
        span_6 = min(span_5, ground_2);
    }
    else
    {
        span_6 = span_5;
    }
    return raymarch_0(vec3<f32>(0.0f, 0.0f, r_10), w_1, sun_2, _S41, span_6, steps_2);
}

@compute
@workgroup_size(8, 8, 1)
fn skyview_main(@builtin(global_invocation_id) id_2 : vec3<u32>)
{
    var _S42 : u32 = id_2.x;
    var _S43 : bool;
    if(_S42 >= u32(192))
    {
        _S43 = true;
    }
    else
    {
        _S43 = (id_2.y) >= u32(108);
    }
    if(_S43)
    {
        return;
    }
    var r_11 : f32 = frame_0.view_0.x;
    var angles_0 : vec2<f32> = skyview_uv_0(r_11, vec2<f32>(f32(_S42) / 191.0f, f32(id_2.y) / 107.0f));
    textureStore((skyview_out_0), (id_2.xy), (vec4<f32>(raymarch_sky_0(r_11, frame_0.view_0.y, angles_0.x, angles_0.y, u32(32)), 1.0f)));
    return;
}

struct SkyVertex_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) ndc_0 : vec2<f32>,
};

@vertex
fn vertex_sky(@builtin(vertex_index) id_3 : u32) -> SkyVertex_0
{
    var ndc_1 : vec2<f32> = vec2<f32>(f32((((id_3 << (u32(1)))) & (u32(2)))), f32((id_3 & (u32(2))))) * vec2<f32>(2.0f) - vec2<f32>(1.0f);
    var output_0 : SkyVertex_0;
    output_0.position_0 = vec4<f32>(ndc_1, 0.0f, 1.0f);
    output_0.ndc_0 = ndc_1;
    return output_0;
}

fn pixel_ray_0( ndc_2 : vec2<f32>) -> vec3<f32>
{
    return normalize(frame_0.forward_0.xyz + frame_0.right_0.xyz * vec3<f32>(ndc_2.x) * vec3<f32>(frame_0.right_0.w) + frame_0.screen_up_0.xyz * vec3<f32>(ndc_2.y) * vec3<f32>(frame_0.screen_up_0.w));
}

fn skyview_coords_0( bottom_6 : f32,  r_12 : f32,  mu_v_1 : f32,  cos_azimuth_1 : f32) -> vec2<f32>
{
    var beta_1 : f32 = acos(clamp(sqrt(max(0.0f, r_12 * r_12 - bottom_6 * bottom_6)) / r_12, -1.0f, 1.0f));
    var zenith_horizon_1 : f32 = 3.14159274101257324f - beta_1;
    var zenith_1 : f32 = acos(clamp(mu_v_1, -1.0f, 1.0f));
    var v_0 : f32;
    if(zenith_1 <= zenith_horizon_1)
    {
        if(zenith_horizon_1 > 0.0f)
        {
            v_0 = 1.0f - sqrt(max(0.0f, 1.0f - zenith_1 / zenith_horizon_1));
        }
        else
        {
            v_0 = 0.0f;
        }
        v_0 = v_0 * 0.5f;
    }
    else
    {
        if(beta_1 > 0.0f)
        {
            v_0 = sqrt(clamp((zenith_1 - zenith_horizon_1) / beta_1, 0.0f, 1.0f));
        }
        else
        {
            v_0 = 0.0f;
        }
        v_0 = 0.5f + v_0 * 0.5f;
    }
    return vec2<f32>(clamp(sqrt(max(0.0f, (1.0f - cos_azimuth_1) * 0.5f)), 0.0f, 1.0f), clamp(v_0, 0.0f, 1.0f));
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) ndc_3 : vec2<f32>,
};

@fragment
fn fragment_sky_inside( _S44 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var bottom_7 : f32 = air_0.shape_0.z;
    var r_13 : f32 = frame_0.view_0.x;
    var dir_1 : vec3<f32> = pixel_ray_0(_S44.ndc_3);
    var up_0 : vec3<f32> = frame_0.eye_0.xyz / vec3<f32>(max(r_13, 1.0f));
    var mu_v_2 : f32 = dot(dir_1, up_0);
    var dir_h_0 : vec3<f32> = dir_1 - vec3<f32>(mu_v_2) * up_0;
    var sun_h_0 : vec3<f32> = frame_0.sun_0.xyz - vec3<f32>(frame_0.view_0.y) * up_0;
    var dir_len_0 : f32 = length(dir_h_0);
    var sun_len_0 : f32 = length(sun_h_0);
    var _S45 : bool;
    if(dir_len_0 > 9.99999997475242708e-07f)
    {
        _S45 = sun_len_0 > 9.99999997475242708e-07f;
    }
    else
    {
        _S45 = false;
    }
    var cos_azimuth_2 : f32;
    if(_S45)
    {
        cos_azimuth_2 = dot(dir_h_0, sun_h_0) / (dir_len_0 * sun_len_0);
    }
    else
    {
        cos_azimuth_2 = 1.0f;
    }
    var uv_3 : vec2<f32> = skyview_coords_0(bottom_7, r_13, mu_v_2, cos_azimuth_2);
    var _S46 : pixelOutput_0 = pixelOutput_0( vec4<f32>((textureSampleLevel((skyview_lut_0), (lut_sampler_0), (vec2<f32>(unit_to_texture_0(uv_3.x, u32(192)), unit_to_texture_0(uv_3.y, u32(108)))), (0.0f))).xyz * vec3<f32>(frame_0.view_0.z), 1.0f) );
    return _S46;
}

struct pixelOutput_1
{
    @location(0) output_2 : vec4<f32>,
};

struct pixelInput_1
{
    @location(0) ndc_4 : vec2<f32>,
};

@fragment
fn fragment_sky_outside( _S47 : pixelInput_1, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_1
{
    var bottom_8 : f32 = air_0.shape_0.z;
    var top_4 : f32 = air_0.shape_0.w;
    var pos_1 : vec3<f32> = frame_0.eye_0.xyz;
    var dir_2 : vec3<f32> = pixel_ray_0(_S47.ndc_4);
    var sun_3 : vec3<f32> = frame_0.sun_0.xyz;
    var r_14 : f32 = length(pos_1);
    var mu_8 : f32 = dot(pos_1, dir_2) / max(r_14, 1.0f);
    var _S48 : f32 = r_14 * r_14;
    var shell2_1 : f32 = (top_4 - bottom_8) * (top_4 + bottom_8);
    var discriminant_1 : f32 = _S48 * mu_8 * mu_8 + (shell2_1 - max(0.0f, _S48 - bottom_8 * bottom_8));
    var _S49 : bool;
    if(discriminant_1 < 0.0f)
    {
        _S49 = true;
    }
    else
    {
        _S49 = mu_8 > 0.0f;
    }
    if(_S49)
    {
        discard;
    }
    var entry_0 : f32 = - r_14 * mu_8 - sqrt(discriminant_1);
    if(entry_0 < 0.0f)
    {
        discard;
    }
    var start_0 : vec3<f32> = pos_1 + vec3<f32>(entry_0) * dir_2;
    var start_r_0 : f32 = length(start_0);
    var start_mu_0 : f32 = dot(start_0, dir_2) / max(start_r_0, 1.0f);
    var span_7 : f32 = distance_to_top_0(start_r_0, start_mu_0, shell2_1, shell2_1);
    var ground_3 : f32 = distance_to_ground_0(start_r_0, start_mu_0, shell2_1);
    var span_8 : f32;
    if(ground_3 >= 0.0f)
    {
        span_8 = min(span_7, ground_3);
    }
    else
    {
        span_8 = span_7;
    }
    var _S50 : pixelOutput_1 = pixelOutput_1( vec4<f32>(raymarch_0(start_0, dir_2, sun_3, shell2_1, span_8, u32(16)) * vec3<f32>(frame_0.view_0.z), 1.0f) );
    return _S50;
}

fn aerial_distance_0( slice_0 : f32) -> f32
{
    var w_2 : f32 = slice_0 / 31.0f;
    var near_0 : f32 = frame_0.eye_0.w;
    return near_0 + (frame_0.view_0.w - near_0) * w_2 * w_2;
}

@compute
@workgroup_size(8, 8, 1)
fn aerial_main(@builtin(global_invocation_id) id_4 : vec3<u32>)
{
    var _S51 : u32 = id_4.x;
    var _S52 : bool;
    if(_S51 >= u32(32))
    {
        _S52 = true;
    }
    else
    {
        _S52 = (id_4.y) >= u32(32);
    }
    if(_S52)
    {
        return;
    }
    var bottom_9 : f32 = air_0.shape_0.z;
    var top_5 : f32 = air_0.shape_0.w;
    var pos_2 : vec3<f32> = frame_0.eye_0.xyz;
    var sun_4 : vec3<f32> = frame_0.sun_0.xyz;
    var _S53 : u32 = id_4.y;
    var dir_3 : vec3<f32> = pixel_ray_0(vec2<f32>(f32(_S51) + 0.5f, f32(_S53) + 0.5f) / vec2<f32>(32.0f) * vec2<f32>(2.0f) - vec2<f32>(1.0f));
    var r_15 : f32 = length(pos_2);
    var mu_9 : f32 = dot(pos_2, dir_3) / max(r_15, 1.0f);
    var _S54 : f32 = bottom_9 * bottom_9;
    var _S55 : f32 = max(0.0f, r_15 * r_15 - _S54);
    var limit_0 : f32 = distance_to_top_0(r_15, mu_9, _S55, (top_5 - bottom_9) * (top_5 + bottom_9));
    var ground_4 : f32 = distance_to_ground_0(r_15, mu_9, _S55);
    var limit_1 : f32;
    if(ground_4 >= 0.0f)
    {
        limit_1 = min(limit_0, ground_4);
    }
    else
    {
        limit_1 = limit_0;
    }
    var cos_theta_3 : f32 = dot(dir_3, sun_4);
    var _S56 : f32 = rayleigh_phase_0(cos_theta_3);
    var _S57 : f32 = mie_phase_0(cos_theta_3, air_0.mie_0.w);
    const _S58 : vec3<f32> = vec3<f32>(1.0f, 1.0f, 1.0f);
    const _S59 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var previous_0 : f32 = 0.0f;
    var slice_1 : u32 = u32(0);
    var throughput_4 : vec3<f32> = _S58;
    var light_2 : vec3<f32> = _S59;
    for(;;)
    {
        if(slice_1 < u32(32))
        {
        }
        else
        {
            break;
        }
        var _S60 : f32 = min(aerial_distance_0(f32(slice_1)), limit_1);
        var _S61 : f32 = max(0.0f, _S60 - previous_0) / 4.0f;
        var k_3 : u32 = u32(0);
        for(;;)
        {
            if(k_3 < u32(4))
            {
            }
            else
            {
                break;
            }
            var t_2 : f32 = previous_0 + (f32(k_3) + 0.5f) * _S61;
            var _S62 : f32 = max(0.0f, _S55 + 2.0f * t_2 * r_15 * mu_9 + t_2 * t_2);
            var radius_2 : f32 = sqrt(max(0.0f, _S62 + _S54));
            var h_7 : f32 = _S62 / (radius_2 + bottom_9);
            var mu_s_here_2 : f32 = dot(pos_2 + vec3<f32>(t_2) * dir_3, sun_4) / max(radius_2, 1.0f);
            var to_sun_2 : vec3<f32>;
            if((distance_to_ground_0(radius_2, mu_s_here_2, _S62)) < 0.0f)
            {
                to_sun_2 = sample_transmittance_0(radius_2, mu_s_here_2);
            }
            else
            {
                to_sun_2 = _S59;
            }
            var d_6 : vec3<f32> = density_0(h_7);
            var sigma_e_2 : vec3<f32> = extinction_0(h_7);
            var sigma_r_1 : vec3<f32> = air_0.rayleigh_0.xyz * vec3<f32>(d_6.x);
            var sigma_m_1 : f32 = air_0.mie_0.x * d_6.y;
            var step_transmittance_2 : vec3<f32> = exp((vec3<f32>(0) - sigma_e_2) * vec3<f32>(_S61));
            var light_3 : vec3<f32> = light_2 + throughput_4 * ((sigma_r_1 * vec3<f32>(_S56) + vec3<f32>((sigma_m_1 * _S57))) * to_sun_2 + (sigma_r_1 + vec3<f32>(sigma_m_1)) * sample_multiscatter_0(radius_2, mu_s_here_2)) * (vec3<f32>(1.0f) - step_transmittance_2) / max(sigma_e_2, vec3<f32>(1.00000000317107685e-30f));
            var throughput_5 : vec3<f32> = throughput_4 * step_transmittance_2;
            k_3 = k_3 + u32(1);
            throughput_4 = throughput_5;
            light_2 = light_3;
        }
        var texel_0 : vec3<u32> = vec3<u32>(_S51, _S53, slice_1);
        textureStore((aerial_inscatter_out_0), (texel_0), (vec4<f32>(light_2 * vec3<f32>(frame_0.view_0.z), 1.0f)));
        textureStore((aerial_transmittance_out_0), (texel_0), (vec4<f32>(throughput_4, 1.0f)));
        var slice_2 : u32 = slice_1 + u32(1);
        previous_0 = _S60;
        slice_1 = slice_2;
    }
    return;
}

fn geometry_distance_0( pixel_0 : vec2<i32>,  dir_4 : vec3<f32>) -> f32
{
    var _S63 : vec3<i32> = vec3<i32>(pixel_0, i32(0));
    var _S64 : f32 = (textureLoad((depth_texture_0), ((_S63)).xy, ((_S63)).z).x);
    if(_S64 <= 0.0f)
    {
        return -1.0f;
    }
    return range_0.depth_0.y / (_S64 + range_0.depth_0.x) / max(dot(dir_4, frame_0.forward_0.xyz), 0.00100000004749745f);
}

fn aerial_coord_0( ndc_5 : vec2<f32>,  distance_0 : f32) -> vec3<f32>
{
    var near_1 : f32 = frame_0.eye_0.w;
    return vec3<f32>(ndc_5.x * 0.5f + 0.5f, ndc_5.y * 0.5f + 0.5f, unit_to_texture_0(sqrt(clamp((distance_0 - near_1) / max(frame_0.view_0.w - near_1, 1.0f), 0.0f, 1.0f)), u32(32)));
}

struct pixelOutput_2
{
    @location(0) output_3 : vec4<f32>,
};

struct pixelInput_2
{
    @location(0) ndc_6 : vec2<f32>,
};

@fragment
fn fragment_aerial_multiply( _S65 : pixelInput_2, @builtin(position) position_3 : vec4<f32>) -> pixelOutput_2
{
    var distance_1 : f32 = geometry_distance_0(vec2<i32>(position_3.xy), pixel_ray_0(_S65.ndc_6));
    if(distance_1 < 0.0f)
    {
        discard;
    }
    var _S66 : pixelOutput_2 = pixelOutput_2( vec4<f32>((textureSampleLevel((aerial_transmittance_lut_0), (lut_sampler_0), (aerial_coord_0(_S65.ndc_6, distance_1)), (0.0f))).xyz, 1.0f) );
    return _S66;
}

struct pixelOutput_3
{
    @location(0) output_4 : vec4<f32>,
};

struct pixelInput_3
{
    @location(0) ndc_7 : vec2<f32>,
};

@fragment
fn fragment_aerial_add( _S67 : pixelInput_3, @builtin(position) position_4 : vec4<f32>) -> pixelOutput_3
{
    var distance_2 : f32 = geometry_distance_0(vec2<i32>(position_4.xy), pixel_ray_0(_S67.ndc_7));
    if(distance_2 < 0.0f)
    {
        discard;
    }
    var _S68 : pixelOutput_3 = pixelOutput_3( vec4<f32>((textureSampleLevel((aerial_inscatter_lut_0), (lut_sampler_0), (aerial_coord_0(_S67.ndc_7, distance_2)), (0.0f))).xyz, 1.0f) );
    return _S68;
}

