//! Проріджування ламаної за екранним критерієм (ROADMAP.md, N2a).
//!
//! Слід росте з ігровим часом, а екран — ні. N1 виміряв, у що це обходиться:
//! 831 тис. вершин, 23.7 мс на кадр, 42 Hz замість 60. Але більшість тих
//! вершин лежить одна на одній: станція на низькій орбіті дає 263 семпли на
//! добу, а з мільярда метрів уся її орбіта — кілька пікселів.
//!
//! ## Критерій виводиться, а не обирається
//!
//! Вузол потрібен там, де без нього хорда відхилилася б від дуги більше ніж на
//! **пів пікселя**. Це не смак: пів пікселя — межа, за якою растеризатор
//! намалює ту саму лінію, тож усе тонше вже не видно нікому.
//!
//! ## Чому в пікселях, а не в метрах
//!
//! Той самий вигин коштує вершини біля камери й не коштує нічого за мільярд
//! метрів. Допуск у метрах довелося б обирати під масштаб, тобто обирати
//! наперед те, що камера вирішує щокадру.
//!
//! ## Чому Дуглас-Пекер, а не жадібний прохід
//!
//! Жадібний прохід («веди хорду, доки лізе») дає інший результат від того, з
//! якого боку йти, і на замкненій орбіті зривається: хорда через повний виток
//! вироджується в точку. Дуглас-Пекер розв'язує рівно те твердження, яким
//! записаний критерій, — **жоден викинутий вузол не відхиляється від хорди
//! більш ніж на допуск**, — і на виродженій хорді працює теж, бо міряє
//! відстань до **відрізка**, а не до прямої.

use engine::camera::Camera;

/// Пів пікселя — межа, за якою растеризатор малює ту саму лінію.
pub const TOLERANCE_PX: f64 = 0.5;

/// Індекси точок, які лишаються після проріджування.
///
/// Точка, яку камера не бачить (позаду неї), проєкції не має; такі точки
/// лишаються всі. Це не обережність заради обережності: ламана, що виходить за
/// спину камері, повертається в кадр з іншого боку, і викидати те, чого не
/// спроєктували, означало б з'єднати два різні місця екрана прямою.
pub fn keep(
    points: &[[f64; 3]],
    camera: &Camera,
    fov_y: f64,
    width: u32,
    height: u32,
    tol_px: f64,
) -> Vec<usize> {
    if points.len() <= 2 {
        return (0..points.len()).collect();
    }

    let screen: Vec<Option<[f64; 2]>> = points
        .iter()
        .map(|&p| {
            camera
                .to_screen(fov_y, width, height, p)
                .map(|px| [f64::from(px[0]), f64::from(px[1])])
        })
        .collect();

    let mut kept = Vec::with_capacity(points.len() / 8 + 2);
    let mut run: Vec<usize> = Vec::new();

    // Ділянка між невидимими точками проріджується сама по собі: усередині неї
    // критерій має сенс, а через розрив — ні.
    for (index, point) in screen.iter().enumerate() {
        match point {
            Some(_) => run.push(index),
            None => {
                flush(&screen, &run, tol_px, &mut kept);
                run.clear();
                kept.push(index);
            }
        }
    }
    flush(&screen, &run, tol_px, &mut kept);

    kept
}

fn flush(screen: &[Option<[f64; 2]>], run: &[usize], tol_px: f64, kept: &mut Vec<usize>) {
    if run.is_empty() {
        return;
    }
    let plane: Vec<[f64; 2]> = run
        .iter()
        .map(|&i| screen[i].expect("у ланцюжку лише видимі точки"))
        .collect();
    for local in simplify(&plane, tol_px) {
        kept.push(run[local]);
    }
}

/// Дуглас-Пекер на площині екрана: індекси точок, без яких ламана не
/// зміниться більш ніж на `tol`.
///
/// Стеком, а не рекурсією: глибина тут — довжина ланки, і ланка з тисячею
/// семплів не має права впертися в стек потоку.
pub fn simplify(points: &[[f64; 2]], tol: f64) -> Vec<usize> {
    let n = points.len();
    if n <= 2 {
        return (0..n).collect();
    }

    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;

    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }

        let mut worst = a;
        let mut worst_px = 0.0;
        for (offset, point) in points[a + 1..b].iter().enumerate() {
            let d = distance_to_segment(*point, points[a], points[b]);
            if d > worst_px {
                worst_px = d;
                worst = a + 1 + offset;
            }
        }

        if worst_px > tol {
            keep[worst] = true;
            stack.push((a, worst));
            stack.push((worst, b));
        }
    }

    (0..n).filter(|&i| keep[i]).collect()
}

/// Відстань від точки до **відрізка** `a`–`b`.
///
/// До відрізка, а не до прямої: на замкненому витку `a` і `b` — та сама точка
/// екрана, прямої там немає, а відстань до точки є й вона правильна.
fn distance_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let length2 = ab[0] * ab[0] + ab[1] * ab[1];

    let t = if length2 <= 0.0 {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / length2).clamp(0.0, 1.0)
    };

    let dx = ap[0] - ab[0] * t;
    let dy = ap[1] - ab[1] * t;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_line_collapses_to_its_ends() {
        let points: Vec<[f64; 2]> = (0..100).map(|i| [i as f64, 0.0]).collect();
        assert_eq!(simplify(&points, TOLERANCE_PX), vec![0, 99]);
    }

    #[test]
    fn a_bend_deeper_than_the_tolerance_survives() {
        let points = [[0.0, 0.0], [50.0, 10.0], [100.0, 0.0]];
        assert_eq!(simplify(&points, TOLERANCE_PX), vec![0, 1, 2]);
    }

    #[test]
    fn a_bend_shallower_than_the_tolerance_does_not() {
        let points = [[0.0, 0.0], [50.0, 0.4], [100.0, 0.0]];
        assert_eq!(simplify(&points, TOLERANCE_PX), vec![0, 2]);
    }

    /// Замкнений виток — той випадок, на якому жадібний прохід зривається:
    /// хорда від першої точки до останньої вироджена.
    #[test]
    fn a_closed_loop_keeps_its_shape() {
        let mut points: Vec<[f64; 2]> = Vec::new();
        for i in 0..=64 {
            let angle = std::f64::consts::TAU * i as f64 / 64.0;
            points.push([100.0 * angle.cos(), 100.0 * angle.sin()]);
        }

        let kept = simplify(&points, TOLERANCE_PX);
        assert!(
            kept.len() > 8 && kept.len() < points.len(),
            "виток із {} точок став {}",
            points.len(),
            kept.len()
        );

        // Форма: жодна викинута точка не далі за допуск від хорди сусідів,
        // які лишились. Це те саме твердження, що й критерій, перевірене
        // прямо, а не через кількість.
        for window in kept.windows(2) {
            for point in &points[window[0] + 1..window[1]] {
                assert!(
                    distance_to_segment(*point, points[window[0]], points[window[1]])
                        <= TOLERANCE_PX,
                    "викинута точка далі за допуск"
                );
            }
        }
    }

    #[test]
    fn the_ends_are_never_dropped() {
        let points = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        let kept = simplify(&points, 1000.0);
        assert_eq!(kept, vec![0, 2]);
    }
}
