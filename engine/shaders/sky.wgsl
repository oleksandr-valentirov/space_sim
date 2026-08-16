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

fn skyview_uv_0( r_7 : f32,  uv_2 : vec2<f32>) -> vec2<f32>
{
    var bottom_3 : f32 = air_0.shape_0.z;
    var beta_0 : f32 = acos(clamp(sqrt(max(0.0f, r_7 * r_7 - bottom_3 * bottom_3)) / r_7, -1.0f, 1.0f));
    var zenith_horizon_0 : f32 = 3.14159274101257324f - beta_0;
    var _S33 : f32 = uv_2.y;
    var zenith_0 : f32;
    if(_S33 < 0.5f)
    {
        var c_1 : f32 = 1.0f - 2.0f * _S33;
        zenith_0 = zenith_horizon_0 * (1.0f - c_1 * c_1);
    }
    else
    {
        var c_2 : f32 = 2.0f * _S33 - 1.0f;
        zenith_0 = zenith_horizon_0 + beta_0 * c_2 * c_2;
    }
    var _S34 : f32 = uv_2.x;
    return vec2<f32>(cos(zenith_0), 1.0f - 2.0f * _S34 * _S34);
}

fn rayleigh_phase_0( cos_theta_0 : f32) -> f32
{
    return 0.05968310311436653f * (1.0f + cos_theta_0 * cos_theta_0);
}

fn mie_phase_0( cos_theta_1 : f32,  g_0 : f32) -> f32
{
    var _S35 : f32 = g_0 * g_0;
    return (1.0f - _S35) / (12.56637096405029297f * pow(max(1.0f + _S35 - 2.0f * g_0 * cos_theta_1, 9.99999997475242708e-07f), 1.5f));
}

fn sample_multiscatter_0( r_8 : f32,  mu_s_1 : f32) -> vec3<f32>
{
    var bottom_4 : f32 = air_0.shape_0.z;
    return (textureSampleLevel((multiscatter_lut_0), (lut_sampler_0), (vec2<f32>(unit_to_texture_0(clamp(mu_s_1 * 0.5f + 0.5f, 0.0f, 1.0f), u32(32)), unit_to_texture_0(clamp((r_8 - bottom_4) / (air_0.shape_0.w - bottom_4), 0.0f, 1.0f), u32(32)))), (0.0f))).xyz;
}

fn raymarch_0( pos_0 : vec3<f32>,  dir_0 : vec3<f32>,  sun_1 : vec3<f32>,  rho2_2 : f32,  span_4 : f32,  steps_1 : u32) -> vec3<f32>
{
    var _S36 : f32 = air_0.shape_0.z;
    var r_9 : f32 = length(pos_0);
    var _S37 : f32 = dot(pos_0, dir_0) / max(r_9, 1.0f);
    var cos_theta_2 : f32 = dot(dir_0, sun_1);
    var _S38 : f32 = rayleigh_phase_0(cos_theta_2);
    var _S39 : f32 = mie_phase_0(cos_theta_2, air_0.mie_0.w);
    var _S40 : f32 = span_4 / f32(steps_1);
    var throughput_1 : vec3<f32> = vec3<f32>(1.0f, 1.0f, 1.0f);
    const _S41 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var light_0 : vec3<f32> = _S41;
    var s_1 : u32 = u32(0);
    for(;;)
    {
        if(s_1 < steps_1)
        {
        }
        else
        {
            break;
        }
        var t_1 : f32 = (f32(s_1) + 0.5f) * _S40;
        var _S42 : f32 = max(0.0f, rho2_2 + 2.0f * t_1 * r_9 * _S37 + t_1 * t_1);
        var radius_1 : f32 = sqrt(max(0.0f, _S42 + _S36 * _S36));
        var h_6 : f32 = _S42 / (radius_1 + _S36);
        var mu_s_here_1 : f32 = dot(pos_0 + vec3<f32>(t_1) * dir_0, sun_1) / max(radius_1, 1.0f);
        var _S43 : vec3<f32>;
        if((distance_to_ground_0(radius_1, mu_s_here_1, _S42)) < 0.0f)
        {
            _S43 = sample_transmittance_0(radius_1, mu_s_here_1);
        }
        else
        {
            _S43 = _S41;
        }
        var _S44 : vec3<f32> = sample_multiscatter_0(radius_1, mu_s_here_1);
        var d_5 : vec3<f32> = density_0(h_6);
        var _S45 : vec3<f32> = extinction_0(h_6);
        var _S46 : vec3<f32> = air_0.rayleigh_0.xyz * vec3<f32>(d_5.x);
        var _S47 : f32 = air_0.mie_0.x * d_5.y;
        var c_3 : u32 = u32(0);
        for(;;)
        {
            if(c_3 < u32(3))
            {
            }
            else
            {
                break;
            }
            var _S48 : u32 = c_3;
            if((_S45[c_3]) <= 0.0f)
            {
                c_3 = c_3 + u32(1);
                continue;
            }
            var step_transmittance_1 : f32 = exp(- _S45[_S48] * _S40);
            light_0[c_3] = light_0[c_3] + throughput_1[c_3] * ((_S46[c_3] * _S38 + _S47 * _S39) * _S43[c_3] + (_S46[c_3] + _S47) * _S44[c_3]) * (1.0f - step_transmittance_1) / _S45[_S48];
            throughput_1[c_3] = throughput_1[c_3] * step_transmittance_1;
            c_3 = c_3 + u32(1);
        }
        s_1 = s_1 + u32(1);
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
    var _S49 : f32 = max(0.0f, r_10 * r_10 - bottom_5 * bottom_5);
    var span_5 : f32 = distance_to_top_0(r_10, mu_v_0, _S49, (top_3 - bottom_5) * (top_3 + bottom_5));
    var ground_1 : f32 = distance_to_ground_0(r_10, mu_v_0, _S49);
    var span_6 : f32;
    if(ground_1 >= 0.0f)
    {
        span_6 = min(span_5, ground_1);
    }
    else
    {
        span_6 = span_5;
    }
    return raymarch_0(vec3<f32>(0.0f, 0.0f, r_10), w_1, sun_2, _S49, span_6, steps_2);
}

@compute
@workgroup_size(8, 8, 1)
fn skyview_main(@builtin(global_invocation_id) id_2 : vec3<u32>)
{
    var _S50 : u32 = id_2.x;
    var _S51 : bool;
    if(_S50 >= u32(192))
    {
        _S51 = true;
    }
    else
    {
        _S51 = (id_2.y) >= u32(108);
    }
    if(_S51)
    {
        return;
    }
    var r_11 : f32 = frame_0.view_0.x;
    var angles_0 : vec2<f32> = skyview_uv_0(r_11, vec2<f32>(f32(_S50) / 191.0f, f32(id_2.y) / 107.0f));
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
fn fragment_sky_inside( _S52 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var bottom_7 : f32 = air_0.shape_0.z;
    var r_13 : f32 = frame_0.view_0.x;
    var dir_1 : vec3<f32> = pixel_ray_0(_S52.ndc_3);
    var up_0 : vec3<f32> = frame_0.eye_0.xyz / vec3<f32>(max(r_13, 1.0f));
    var mu_v_2 : f32 = dot(dir_1, up_0);
    var dir_h_0 : vec3<f32> = dir_1 - vec3<f32>(mu_v_2) * up_0;
    var sun_h_0 : vec3<f32> = frame_0.sun_0.xyz - vec3<f32>(frame_0.view_0.y) * up_0;
    var dir_len_0 : f32 = length(dir_h_0);
    var sun_len_0 : f32 = length(sun_h_0);
    var _S53 : bool;
    if(dir_len_0 > 9.99999997475242708e-07f)
    {
        _S53 = sun_len_0 > 9.99999997475242708e-07f;
    }
    else
    {
        _S53 = false;
    }
    var cos_azimuth_2 : f32;
    if(_S53)
    {
        cos_azimuth_2 = dot(dir_h_0, sun_h_0) / (dir_len_0 * sun_len_0);
    }
    else
    {
        cos_azimuth_2 = 1.0f;
    }
    var uv_3 : vec2<f32> = skyview_coords_0(bottom_7, r_13, mu_v_2, cos_azimuth_2);
    var _S54 : pixelOutput_0 = pixelOutput_0( vec4<f32>((textureSampleLevel((skyview_lut_0), (lut_sampler_0), (vec2<f32>(unit_to_texture_0(uv_3.x, u32(192)), unit_to_texture_0(uv_3.y, u32(108)))), (0.0f))).xyz * vec3<f32>(frame_0.view_0.z), 1.0f) );
    return _S54;
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
fn fragment_sky_outside( _S55 : pixelInput_1, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_1
{
    var bottom_8 : f32 = air_0.shape_0.z;
    var top_4 : f32 = air_0.shape_0.w;
    var pos_1 : vec3<f32> = frame_0.eye_0.xyz;
    var dir_2 : vec3<f32> = pixel_ray_0(_S55.ndc_4);
    var sun_3 : vec3<f32> = frame_0.sun_0.xyz;
    var r_14 : f32 = length(pos_1);
    var mu_8 : f32 = dot(pos_1, dir_2) / max(r_14, 1.0f);
    var _S56 : f32 = r_14 * r_14;
    var shell2_1 : f32 = (top_4 - bottom_8) * (top_4 + bottom_8);
    var discriminant_1 : f32 = _S56 * mu_8 * mu_8 + (shell2_1 - max(0.0f, _S56 - bottom_8 * bottom_8));
    var _S57 : bool;
    if(discriminant_1 < 0.0f)
    {
        _S57 = true;
    }
    else
    {
        _S57 = mu_8 > 0.0f;
    }
    if(_S57)
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
    var ground_2 : f32 = distance_to_ground_0(start_r_0, start_mu_0, shell2_1);
    var span_8 : f32;
    if(ground_2 >= 0.0f)
    {
        span_8 = min(span_7, ground_2);
    }
    else
    {
        span_8 = span_7;
    }
    var _S58 : pixelOutput_1 = pixelOutput_1( vec4<f32>(raymarch_0(start_0, dir_2, sun_3, shell2_1, span_8, u32(16)) * vec3<f32>(frame_0.view_0.z), 1.0f) );
    return _S58;
}

