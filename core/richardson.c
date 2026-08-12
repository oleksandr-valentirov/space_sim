#include "richardson.h"

#include "cr3bp.h"

/* Only ever used to turn an angular rate into a period. Written out because
 * M_PI is not standard C and the value is a constant of the problem, not
 * something to obtain from a header that may or may not define it. */
#define RICHARDSON_TWO_PI 6.28318530717958647692

/* The c_n of the expansion, for n = 2, 3, 4.
 *
 * gamma is the distance from the libration point to its nearer primary, in
 * units of the primaries' separation. The two point cases differ in more than
 * a sign: for L1 the alternating factor sits on the far primary's term, for L2
 * on the whole expression. That is the coordinate direction changing, and it
 * is why these are written out separately rather than parameterised. */
static void coefficients_c(double mu, int point, double gamma, double c[5])
{
    double g3 = gamma * gamma * gamma;
    double base = point == 1 ? 1.0 - gamma : 1.0 + gamma;

    /* Running powers, so no pow() and no repeated multiplication chains:
     * gamma^(n+1) and base^(n+1), both starting at the cube. */
    double gp = g3;
    double bp = base * base * base;

    for (int n = 2; n <= 4; n++) {
        double sign = (n % 2 == 0) ? 1.0 : -1.0;

        if (point == 1) {
            c[n] = (1.0 / g3) * (mu + sign * (1.0 - mu) * gp / bp);
        } else {
            c[n] = (sign / g3) * (mu + (1.0 - mu) * gp / bp);
        }

        gp *= gamma;
        bp *= base;
    }
}

CoreResult richardson_halo(double mu, int point, double az,
                           State *out, double *period)
{
    if (out == NULL || period == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(mu > 0.0) || !(mu < 1.0) || az == 0.0) {
        return CORE_ERR_INVALID_ARG;
    }
    if (point != 1 && point != 2) {
        return CORE_ERR_INVALID_ARG;
    }

    Vec3d li;
    if (cr3bp_lagrange(mu, point, &li) != CORE_OK) {
        return CORE_ERR_INVALID_ARG;
    }

    double gamma = point == 1 ? (1.0 - mu) - li.x : li.x - (1.0 - mu);
    if (!(gamma > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    double c[5];
    coefficients_c(mu, point, gamma, c);
    double c2 = c[2], c3 = c[3], c4 = c[4];

    /* The in-plane linear frequency, from lambda^4 + (c2-2)lambda^2
     * - (c2-1)(1+2c2) = 0, and the amplitude ratio that goes with it. */
    double disc = 9.0 * c2 * c2 - 8.0 * c2;
    if (!(disc > 0.0)) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    double lam2 = (c2 - 2.0 + sqrt(disc)) / 2.0;
    if (!(lam2 > 0.0)) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }
    double lam = sqrt(lam2);
    double k = (lam2 + 1.0 + 2.0 * c2) / (2.0 * lam);
    double k2 = k * k;

    double d1 = (3.0 * lam2 / k) * (k * (6.0 * lam2 - 1.0) - 2.0 * lam);
    double d2 = (8.0 * lam2 / k) * (k * (11.0 * lam2 - 1.0) - 2.0 * lam);
    if (d1 == 0.0 || d2 == 0.0) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    /* Second-order coefficients. */
    double a21 = 3.0 * c3 * (k2 - 2.0) / (4.0 * (1.0 + 2.0 * c2));
    double a22 = 3.0 * c3 / (4.0 * (1.0 + 2.0 * c2));
    double a23 = -(3.0 * c3 * lam / (4.0 * k * d1))
                 * (3.0 * k2 * k * lam - 6.0 * k * (k - lam) + 4.0);
    double a24 = -(3.0 * c3 * lam / (4.0 * k * d1)) * (2.0 + 3.0 * k * lam);
    double b21 = -(3.0 * c3 * lam / (2.0 * d1)) * (3.0 * k * lam - 4.0);
    double b22 = 3.0 * c3 * lam / d1;
    double d21 = -c3 / (2.0 * lam2);

    /* Third-order coefficients. */
    double a31 = -(9.0 * lam / (4.0 * d2))
                     * (4.0 * c3 * (k * a23 - b21) + k * c4 * (4.0 + k2))
               + ((9.0 * lam2 + 1.0 - c2) / (2.0 * d2))
                     * (3.0 * c3 * (2.0 * a23 - k * b21)
                        + c4 * (2.0 + 3.0 * k2));
    double a32 = -(1.0 / d2)
                 * ((9.0 * lam / 4.0) * (4.0 * c3 * (k * a24 - b22) + k * c4)
                    + 1.5 * (9.0 * lam2 + 1.0 - c2)
                          * (c3 * (k * b22 + d21 - 2.0 * a24) - c4));
    double b31 = (3.0 / (8.0 * d2))
                 * (8.0 * lam * (3.0 * c3 * (k * b21 - 2.0 * a23)
                                 - c4 * (2.0 + 3.0 * k2))
                    + (9.0 * lam2 + 1.0 + 2.0 * c2)
                          * (4.0 * c3 * (k * a23 - b21) + k * c4 * (4.0 + k2)));
    double b32 = (1.0 / d2)
                 * (9.0 * lam * (c3 * (k * b22 + d21 - 2.0 * a24) - c4)
                    + 0.375 * (9.0 * lam2 + 1.0 + 2.0 * c2)
                          * (4.0 * c3 * (k * a24 - b22) + k * c4));
    double d31 = (3.0 / (64.0 * lam2)) * (4.0 * c3 * a24 + c4);
    double d32 = (3.0 / (64.0 * lam2))
                 * (4.0 * c3 * (a23 - d21) + c4 * (4.0 + k2));

    /* The frequency correction and the amplitude constraint. */
    double den = 2.0 * lam * (lam * (1.0 + k2) - 2.0 * k);
    if (den == 0.0) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    double s1 = (1.5 * c3 * (2.0 * a21 * (k2 - 2.0) - a23 * (k2 + 2.0)
                             - 2.0 * k * b21)
                 - 0.375 * c4 * (3.0 * k2 * k2 - 8.0 * k2 + 8.0)) / den;
    double s2 = (1.5 * c3 * (2.0 * a22 * (k2 - 2.0) + a24 * (k2 + 2.0)
                             + 2.0 * k * b22 + 5.0 * d21)
                 + 0.375 * c4 * (12.0 - k2)) / den;

    double a1 = -1.5 * c3 * (2.0 * a21 + a23 + 5.0 * d21)
                - 0.375 * c4 * (12.0 - k2);
    double a2 = 1.5 * c3 * (a24 - 2.0 * a21) + 1.125 * c4;

    double l1 = a1 + 2.0 * lam2 * s1;
    double l2 = a2 + 2.0 * lam2 * s2;
    double delta = lam2 - c2;

    /* l1 Ax^2 + l2 Az^2 + delta = 0 is what ties the two amplitudes together,
     * and what makes a halo a halo rather than an arbitrary out-of-plane
     * wobble. Beyond a certain Az it has no positive root, and that is the
     * honest end of this approximation's reach. */
    double amp_z = (az < 0.0 ? -az : az) / gamma;
    if (l1 == 0.0) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    double amp_x_sq = -(delta + l2 * amp_z * amp_z) / l1;
    if (!(amp_x_sq > 0.0)) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }
    double amp_x = sqrt(amp_x_sq);

    double omega = 1.0 + s1 * amp_x_sq + s2 * amp_z * amp_z;
    if (!(omega > 0.0)) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }
    *period = RICHARDSON_TWO_PI / (lam * omega);

    /* Evaluated at the perpendicular crossing, where cos(tau) = -1,
     * cos(2 tau) = 1, cos(3 tau) = -1 and every sine is zero. That is the
     * crossing the JPL catalogue publishes, and choosing it rather than the
     * one half a period away is what puts the returned velocity on the same
     * branch as the catalogue's. It is also what removes libm; see the header.
     *
     * dn carries the branch. At this crossing the leading z term is -dn Az, so
     * dn is the opposite sign to the z the caller asked for. */
    double dn = az > 0.0 ? -1.0 : 1.0;
    double ax2 = amp_x_sq;
    double az2 = amp_z * amp_z;

    double x = a21 * ax2 + a22 * az2 + amp_x
             + (a23 * ax2 - a24 * az2)
             - (a31 * amp_x * ax2 - a32 * amp_x * az2);

    double z = -dn * amp_z
             - 2.0 * dn * d21 * amp_x * amp_z
             - dn * (d32 * amp_z * ax2 - d31 * amp_z * az2);

    /* d/d(tau) of the y series, which the sines make the only nonzero
     * velocity component here. */
    double dy = -k * amp_x
              + 2.0 * (b21 * ax2 - b22 * az2)
              - 3.0 * (b31 * amp_x * ax2 - b32 * amp_x * az2);

    /* Out of the libration point's scaled frame and into the synodic one. The
     * x axis runs the same way in both, for L1 and L2 alike; only gamma and
     * the c_n differ between the points. */
    out->r = vec3(li.x + gamma * x, 0.0, gamma * z);
    out->v = vec3(0.0, gamma * lam * omega * dy, 0.0);
    out->t = 0.0;

    return CORE_OK;
}
