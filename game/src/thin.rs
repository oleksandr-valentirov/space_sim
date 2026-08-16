//! Polyline thinning by a screen criterion (ROADMAP.md, N2a).
//!
//! The trail grows with game time; the screen does not. N1 measured the cost:
//! 831 thousand vertices, 23.7 ms per frame, 42 Hz instead of 60. But most of
//! those vertices lie on top of each other: a station in low orbit gives 263
//! samples per day, and from a billion metres its whole orbit is a few pixels.
//!
//! ## The criterion is derived, not chosen
//!
//! A node is needed where without it the chord would deviate from the arc by
//! more than **half a pixel**. Not a taste: half a pixel is the limit beyond
//! which the rasteriser draws the same line, so anything finer is visible to
//! nobody.
//!
//! ## Who sets the tolerance
//!
//! Not this module: there is only the algorithm here, and the tolerance
//! arrives from outside. `crate::trail` converts half a pixel into metres, and
//! that is where it is recorded why metres rather than screen pixels: in
//! metres the tolerance does not depend on view direction, and the cache
//! survives rotating the camera.
//!
//! ## Why Douglas-Peucker rather than a greedy pass
//!
//! A greedy pass ("extend the chord while it fits") gives different results
//! depending on which end you start from, and breaks down on a closed orbit:
//! the chord across a full revolution degenerates to a point. Douglas-Peucker
//! solves exactly the statement the criterion is written as -- **no discarded
//! node deviates from the chord by more than the tolerance** -- and works on a
//! degenerate chord too, because it measures distance to a **segment** rather
//! than to a line.

/// Half a pixel: the limit beyond which the rasteriser draws the same line.
pub const TOLERANCE_PX: f64 = 0.5;

/// The same in the screen plane.
///
/// A wrapper over [`simplify3`] rather than a second copy of the algorithm:
/// `z = 0` makes the three-dimensional distance to a segment exactly
/// two-dimensional, and two copies of Douglas-Peucker would quietly
/// diverge.
pub fn simplify(points: &[[f64; 2]], tol: f64) -> Vec<usize> {
    let lifted: Vec<[f64; 3]> = points.iter().map(|p| [p[0], p[1], 0.0]).collect();
    simplify3(&lifted, tol)
}

/// Douglas-Peucker: the indices of points without which the polyline changes
/// by no more than
/// `tol`.
///
/// By a stack rather than recursion: the depth here is the leg's length, and a
/// leg of a thousand samples has no business hitting the thread's stack.
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

/// Distance from a point to the **segment** `a`-`b`.
///
/// To the segment rather than the line: on a closed revolution `a` and `b` are
/// the same point, there is no line there, and the distance to a point exists
/// and is correct.
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

    /// A closed revolution -- the case a greedy pass breaks down on: the
    /// chord from the first point to the last is degenerate.
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
            "a revolution of {} points became {}",
            points.len(),
            kept.len()
        );

        // Shape: no discarded point is further than the tolerance from the
        // chord of the neighbours that remain. The same statement as the
        // criterion, checked directly rather than through a count.
        let lift = |p: [f64; 2]| [p[0], p[1], 0.0];
        for window in kept.windows(2) {
            for point in &points[window[0] + 1..window[1]] {
                assert!(
                    distance_to_segment(
                        lift(*point),
                        lift(points[window[0]]),
                        lift(points[window[1]])
                    ) <= TOLERANCE_PX,
                    "a discarded point is further than the tolerance"
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
