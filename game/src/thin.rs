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
//! ## Хто задає допуск
//!
//! Не цей модуль: тут лише алгоритм, а допуск приходить ззовні. Пів пікселя
//! перераховує в метри `crate::trail`, і саме там записано, чому в метрах, а
//! не в пікселях екрана: у метрах допуск не залежить від напрямку погляду, і
//! кеш переживає обертання камери.
//!
//! ## Чому Дуглас-Пекер, а не жадібний прохід
//!
//! Жадібний прохід («веди хорду, доки лізе») дає інший результат від того, з
//! якого боку йти, і на замкненій орбіті зривається: хорда через повний виток
//! вироджується в точку. Дуглас-Пекер розв'язує рівно те твердження, яким
//! записаний критерій, — **жоден викинутий вузол не відхиляється від хорди
//! більш ніж на допуск**, — і на виродженій хорді працює теж, бо міряє
//! відстань до **відрізка**, а не до прямої.

/// Пів пікселя — межа, за якою растеризатор малює ту саму лінію.
pub const TOLERANCE_PX: f64 = 0.5;

/// Те саме на площині екрана.
///
/// Обгортка над [`simplify3`], а не друга копія алгоритму: `z = 0` робить
/// тривимірну відстань до відрізка рівно двовимірною, а дві копії
/// Дугласа-Пекера тихо розійшлися б.
pub fn simplify(points: &[[f64; 2]], tol: f64) -> Vec<usize> {
    let lifted: Vec<[f64; 3]> = points.iter().map(|p| [p[0], p[1], 0.0]).collect();
    simplify3(&lifted, tol)
}

/// Дуглас-Пекер: індекси точок, без яких ламана не зміниться більш ніж на
/// `tol`.
///
/// Стеком, а не рекурсією: глибина тут — довжина ланки, і ланка з тисячею
/// семплів не має права впертися в стек потоку.
pub fn simplify3(points: &[[f64; 3]], tol: f64) -> Vec<usize> {
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
/// До відрізка, а не до прямої: на замкненому витку `a` і `b` — та сама точка,
/// прямої там немає, а відстань до точки є й вона правильна.
fn distance_to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
    let length2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];

    let t = if length2 <= 0.0 {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / length2).clamp(0.0, 1.0)
    };

    let d = [ap[0] - ab[0] * t, ap[1] - ab[1] * t, ap[2] - ab[2] * t];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
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
        let lift = |p: [f64; 2]| [p[0], p[1], 0.0];
        for window in kept.windows(2) {
            for point in &points[window[0] + 1..window[1]] {
                assert!(
                    distance_to_segment(
                        lift(*point),
                        lift(points[window[0]]),
                        lift(points[window[1]])
                    ) <= TOLERANCE_PX,
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
