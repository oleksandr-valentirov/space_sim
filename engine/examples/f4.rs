//! A sweep for ROADMAP F4: where exactly the naive path breaks.

fn main() {
    let gpu = engine::gpu::Gpu::new(wgpu::Instance::default(), None).unwrap();
    let step = 0.1;

    println!("Object 10 m from the camera, both at distance D from the origin.");
    println!("The camera moves in steps of {step} m; that is ~1.2 pixels per step.\n");
    println!(
        "{:>10} {:>12} {:>22} {:>22}",
        "D, m", "ULP f32, m", "camera-relative", "naive"
    );
    println!(
        "{:>10} {:>12} {:>22} {:>22}",
        "", "", "seen, mean/max px", "seen, mean/max px"
    );

    for exponent in [3, 5, 7, 8, 9, 11] {
        let d = 10f64.powi(exponent);
        let ulp = f64::from(f32::EPSILON) * d;

        let mut cells = Vec::new();
        for relative in [true, false] {
            let steps = engine::camera_probe::sweep_at(&gpu, 256, relative, 12, step, d).unwrap();
            let seen = steps.iter().filter(|s| s.visible > 0).count();
            let shifts: Vec<f64> = steps.iter().skip(1).map(|s| s.shift).collect();
            let mean = shifts.iter().sum::<f64>() / shifts.len() as f64;
            let max = shifts.iter().fold(0.0f64, |a, &b| a.max(b));
            cells.push(if seen == 0 {
                "not in frame".to_string()
            } else {
                format!("{seen}/12, {mean:.2}/{max:.2}")
            });
        }

        println!("{d:>10.0e} {ulp:>12.3} {:>22} {:>22}", cells[0], cells[1]);
    }
}
