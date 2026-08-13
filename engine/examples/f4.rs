//! Розгортка для ROADMAP F4: де саме наївний шлях ламається.

fn main() {
    let gpu = engine::gpu::Gpu::new(wgpu::Instance::default(), None).unwrap();
    let step = 0.1;

    println!("Об'єкт за 10 м від камери, обидва — на відстані D від початку координат.");
    println!("Камера рухається кроками по {step} м; це ~1.2 пікселя на крок.\n");
    println!(
        "{:>10} {:>12} {:>22} {:>22}",
        "D, м", "ULP f32, м", "camera-relative", "наївний"
    );
    println!(
        "{:>10} {:>12} {:>22} {:>22}",
        "", "", "видно, сер/макс px", "видно, сер/макс px"
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
                "немає в кадрі".to_string()
            } else {
                format!("{seen}/12, {mean:.2}/{max:.2}")
            });
        }

        println!("{d:>10.0e} {ulp:>12.3} {:>22} {:>22}", cells[0], cells[1]);
    }
}
