//! Обидва фрейми траєкторії справді малюють щось на екрані (ROADMAP F6).
//!
//! Не «на око»: перша версія цього рендера мовчки давала порожній кадр
//! рівно для обертового фрейму — пайплайн збирався, `draw` виконувався без
//! жодної помилки чи попередження від wgpu, і єдиним симптомом був чорний
//! PNG. Причина лишилась не до кінця з'ясованою (`trajectory.slang`,
//! коментар над двома точками входу), а цей тест — застава від регресії
//! того самого класу: раз він ловить порожній кадр, шейдер можна міняти
//! не передивляючись знімки вручну щоразу.

use engine::gpu::Gpu;
use engine::trajectory;
use engine::trajectory_render::{geocentric_framing, render, rotating_framing, Params};

const SIZE: u32 = 256;

fn lit_pixels(shot: &engine::shot::Shot) -> u64 {
    let mut count = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if p[0] > 5 || p[1] > 5 || p[2] > 5 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn both_frames_draw_visible_pixels() {
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu");
        return;
    };

    let samples = trajectory::load();

    let geocentric = render(
        &gpu,
        SIZE,
        SIZE,
        &samples,
        &Params {
            rotating: false,
            framing: geocentric_framing(&samples),
            colour: [0.9, 0.6, 0.2, 1.0],
        },
    )
    .expect("геоцентричний рендер мав пройти");

    let rotating = render(
        &gpu,
        SIZE,
        SIZE,
        &samples,
        &Params {
            rotating: true,
            framing: rotating_framing(&samples),
            colour: [0.3, 0.8, 0.9, 1.0],
        },
    )
    .expect("обертовий рендер мав пройти");

    assert!(
        lit_pixels(&geocentric) > 100,
        "геоцентричний кадр майже порожній: {} пікселів",
        lit_pixels(&geocentric)
    );
    assert!(
        lit_pixels(&rotating) > 100,
        "обертовий кадр майже порожній: {} пікселів — саме так виглядав баг \
         із двома точками входу до виправлення",
        lit_pixels(&rotating)
    );
}
