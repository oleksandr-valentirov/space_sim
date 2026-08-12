/* The state transition matrix (ROADMAP C2b).
 *
 * ROADMAP C2b names checking the STM by finite differences "the most useful
 * tool of the whole stage", and the reason is diagnostic rather than numeric:
 * when differential correction later fails to converge, this test is what
 * tells you whether the fault is in the variational equations or in the
 * correction step. So it is written before the corrector, not after it.
 *
 * Three independent checks, and they fail differently, which is the point:
 *
 *   - finite differences catch a wrong Jacobian entry;
 *   - symplecticity catches a sign error, and needs no reference at all;
 *   - composition catches an error in how the matrix is accumulated in time.
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "refdata.h"
#include "stm.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16

static RefHalo orbit[MAX_ORBITS];
static size_t n_orbits;
static double mu;
static Cr3bpCtx ctx;

static Dop853Config tight(void)
{
    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-14;
    cfg.max_steps = 20000000;
    return cfg;
}

/* Propagate from s to t_end, optionally collecting the STM. */
static CoreResult propagate(const State *s, double t_end, State *out,
                            double phi[STM_SIZE])
{
    Dop853Config cfg = tight();
    Dop853State st;
    memset(&st, 0, sizeof st);

    return stm_integrate(accel_cr3bp_var, &ctx, s, t_end, &cfg, &st, out, phi);
}

/* Component j of a state, in the STM's ordering. */
static double component(const State *s, int j)
{
    switch (j) {
    case 0:  return s->r.x;
    case 1:  return s->r.y;
    case 2:  return s->r.z;
    case 3:  return s->v.x;
    case 4:  return s->v.y;
    default: return s->v.z;
    }
}

static void offset(State *s, int j, double eps)
{
    switch (j) {
    case 0:  s->r.x += eps; break;
    case 1:  s->r.y += eps; break;
    case 2:  s->r.z += eps; break;
    case 3:  s->v.x += eps; break;
    case 4:  s->v.y += eps; break;
    default: s->v.z += eps; break;
    }
}

/* Largest absolute disagreement between a column of Phi and the same column
 * measured by central differences. Returns -1 on an integration failure. */
static double finite_difference_defect(const State *s0, double t_end,
                                       double eps, double *out_scale)
{
    double phi[STM_SIZE];
    State end;
    if (propagate(s0, t_end, &end, phi) != CORE_OK) {
        return -1.0;
    }

    double worst = 0.0;
    double scale = 0.0;

    for (int j = 0; j < 6; j++) {
        State plus = *s0, minus = *s0;
        offset(&plus, j, eps);
        offset(&minus, j, -eps);

        State end_plus, end_minus;
        if (propagate(&plus, t_end, &end_plus, NULL) != CORE_OK ||
            propagate(&minus, t_end, &end_minus, NULL) != CORE_OK) {
            return -1.0;
        }

        for (int i = 0; i < 6; i++) {
            double numeric = (component(&end_plus, i) - component(&end_minus, i))
                             / (2.0 * eps);
            double d = fabs(numeric - phi[i * 6 + j]);
            if (d > worst) {
                worst = d;
            }
            if (fabs(numeric) > scale) {
                scale = fabs(numeric);
            }
        }
    }

    if (out_scale != NULL) {
        *out_scale = scale;
    }
    return worst;
}

/* The Hessian on its own, without any integration in the way: differentiate
 * accel_cr3bp in position and compare. Separating this from the trajectory
 * test is what makes a failure legible - a wrong Hessian entry fails here
 * with no integrator involved. */
static void check_hessian_at(Vec3d r)
{
    double u[9];
    cr3bp_hessian(r, mu, u);

    const double eps = 1e-6;

    for (int j = 0; j < 3; j++) {
        Vec3d rp = r, rm = r;
        double *pp = j == 0 ? &rp.x : (j == 1 ? &rp.y : &rp.z);
        double *pm = j == 0 ? &rm.x : (j == 1 ? &rm.y : &rm.z);
        *pp += eps;
        *pm -= eps;

        Vec3d ap, am;
        accel_cr3bp(0.0, rp, vec3_zero(), &ctx, &ap);
        accel_cr3bp(0.0, rm, vec3_zero(), &ctx, &am);

        double numeric[3] = {
            (ap.x - am.x) / (2.0 * eps),
            (ap.y - am.y) / (2.0 * eps),
            (ap.z - am.z) / (2.0 * eps),
        };

        for (int i = 0; i < 3; i++) {
            double magnitude = fabs(u[i * 3 + j]);
            double allowed = 1e-4 * (magnitude > 1.0 ? magnitude : 1.0);
            CHECK(fabs(numeric[i] - u[i * 3 + j]) < allowed);
        }
    }
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit,
                          MAX_ORBITS, &n_orbits) != CORE_OK ||
        refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }
    ctx.mu = mu;

    /* The Hessian, at points with no symmetry to hide a transposed index. */
    check_hessian_at(vec3(0.3, 0.4, 0.2));
    check_hessian_at(vec3(-0.7, 0.15, -0.3));
    check_hessian_at(vec3(1.05, 0.0, -0.2));

    /* Symmetry is a property of a Hessian, not something the caller should
     * have to assume. */
    {
        double u[9];
        cr3bp_hessian(vec3(0.3, 0.4, 0.2), mu, u);
        CHECK_BITS_EQ(u[1], u[3]);
        CHECK_BITS_EQ(u[2], u[6]);
        CHECK_BITS_EQ(u[5], u[7]);
    }

    /* Zero duration gives the identity, exactly. */
    {
        double phi[STM_SIZE];
        State end;
        State s = orbit[0].s;
        CHECK(propagate(&s, s.t, &end, phi) == CORE_OK);

        double eye[STM_SIZE];
        stm_identity(eye);
        for (int i = 0; i < STM_SIZE; i++) {
            CHECK_BITS_EQ(phi[i], eye[i]);
        }
        CHECK(stm_symplectic_defect(phi) == 0.0);
    }

    /* Finite differences over half a period and a full period.
     *
     * The threshold is set by the measurement, not by the STM. Perturbed
     * trajectories take their own adaptive step sequences, so their difference
     * carries noise of order tol/eps = 1e-14/1e-6 = 1e-8, and no agreement
     * closer than that is observable by this method. Measured worst
     * disagreement: 2.6e-08 at half a period and 5.3e-07 at a full one, on
     * columns whose entries reach 3.0 and 12.7 respectively. */
    for (size_t i = 0; i < 4; i++) {
        double scale = 0.0;
        double half = finite_difference_defect(&orbit[i].s,
                                               0.5 * orbit[i].period,
                                               1e-6, &scale);
        CHECK(half >= 0.0);
        CHECK(scale > 1.0);
        CHECK(half < 1e-5 * scale);
    }

    {
        double scale = 0.0;
        double full = finite_difference_defect(&orbit[0].s, orbit[0].period,
                                               1e-6, &scale);
        CHECK(full >= 0.0);
        CHECK(full < 1e-5 * scale);
    }

    /* Symplecticity, which needs no reference orbit and holds for any
     * trajectory of any duration - once the matrix is in the canonical
     * coordinates the property is actually stated in. See cr3bp_stm_canonical
     * for what happens if it is not. Checked on the most unstable orbit in the
     * fixture too: there Phi has entries above 500, so the defect being small
     * is a statement about relative accuracy, not about small numbers.
     * Measured at tolerance 1e-14: 7.2e-12 for orbit 0 over one period,
     * 4.2e-11 for orbit 3 against entries of 531. */
    for (size_t i = 0; i < 4; i++) {
        double phi[STM_SIZE], canonical[STM_SIZE];
        State end;
        CHECK(propagate(&orbit[i].s, orbit[i].period, &end, phi) == CORE_OK);
        cr3bp_stm_canonical(phi, canonical);

        double biggest = 0.0;
        for (int k = 0; k < STM_SIZE; k++) {
            if (fabs(canonical[k]) > biggest) {
                biggest = fabs(canonical[k]);
            }
        }
        CHECK(stm_symplectic_defect(canonical) < 1e-10 * biggest);
    }

    /* And the defect is the integrator's, not a defect in the equations: it
     * falls with the tolerance. That distinction is the whole diagnostic value
     * of the check. Measured for orbit 0: 7.5e-09, 4.4e-10, 7.2e-12 at
     * tolerances 1e-12, 1e-13, 1e-14. */
    {
        double defect[3];
        double tolerances[3] = { 1e-12, 1e-13, 1e-14 };

        for (int i = 0; i < 3; i++) {
            Dop853Config cfg = tight();
            cfg.tol_m = tolerances[i];
            Dop853State st;
            memset(&st, 0, sizeof st);

            double phi[STM_SIZE], canonical[STM_SIZE];
            State end;
            CHECK(stm_integrate(accel_cr3bp_var, &ctx, &orbit[0].s,
                                orbit[0].period, &cfg, &st, &end, phi)
                  == CORE_OK);
            cr3bp_stm_canonical(phi, canonical);
            defect[i] = stm_symplectic_defect(canonical);
        }

        CHECK(defect[1] < defect[0] / 5.0);
        CHECK(defect[2] < defect[1] / 5.0);
    }

    /* The change of variables is exactly invertible, so a round trip through
     * it is the identity to the last bit. */
    {
        double a[STM_SIZE], eye[STM_SIZE], canonical[STM_SIZE];
        stm_identity(eye);
        cr3bp_stm_canonical(eye, canonical);
        for (int k = 0; k < STM_SIZE; k++) {
            CHECK_BITS_EQ(canonical[k], eye[k]);
        }

        for (int k = 0; k < STM_SIZE; k++) {
            a[k] = (double)(k + 1);
        }
        cr3bp_stm_canonical(a, canonical);
        CHECK(stm_symplectic_defect(a) != stm_symplectic_defect(canonical));
    }

    /* Carrying the STM does not change the trajectory - bit for bit.
     *
     * This is not a nicety, it is what the corrector in correct.c is built on.
     * It finds the crossing of y = 0 with cheap one-block propagations and
     * then measures the sensitivities there with a seven-block one, and those
     * two are the same trajectory only if this holds. It does hold by
     * construction: block 0's arithmetic does not depend on how many blocks
     * travel beside it, and the step controller reads block 0 alone. Asserted
     * anyway, because "by construction" is a claim about code that changes. */
    for (size_t i = 0; i < 4; i++) {
        Dop853Config cfg = tight();

        Dop853State st_one;
        memset(&st_one, 0, sizeof st_one);
        State plain;
        CHECK(dop853_integrate(accel_cr3bp, &ctx, &orbit[i].s,
                               0.5 * orbit[i].period, &cfg, &st_one, &plain)
              == CORE_OK);

        Dop853State st_seven;
        memset(&st_seven, 0, sizeof st_seven);
        State with_stm;
        double phi[STM_SIZE];
        CHECK(stm_integrate(accel_cr3bp_var, &ctx, &orbit[i].s,
                            0.5 * orbit[i].period, &cfg, &st_seven, &with_stm,
                            phi) == CORE_OK);

        CHECK_BITS_EQ(plain.r.x, with_stm.r.x);
        CHECK_BITS_EQ(plain.r.y, with_stm.r.y);
        CHECK_BITS_EQ(plain.r.z, with_stm.r.z);
        CHECK_BITS_EQ(plain.v.x, with_stm.v.x);
        CHECK_BITS_EQ(plain.v.y, with_stm.v.y);
        CHECK_BITS_EQ(plain.v.z, with_stm.v.z);
        CHECK_BITS_EQ(st_one.h, st_seven.h);
        CHECK(st_one.n_accepted == st_seven.n_accepted);
        CHECK(st_one.n_rejected == st_seven.n_rejected);
    }

    /* Composition: propagating in two legs and multiplying must agree with
     * propagating in one. This is the property multiple shooting in C4 rests
     * on, and it is independent of whether the Jacobian is right - a wrong but
     * consistent Jacobian would still compose. */
    {
        State s0 = orbit[0].s;
        double t_mid = 0.5 * orbit[0].period;

        double phi_a[STM_SIZE], phi_b[STM_SIZE], phi_ab[STM_SIZE];
        State mid, end_two, end_one;

        CHECK(propagate(&s0, t_mid, &mid, phi_a) == CORE_OK);
        CHECK(propagate(&mid, orbit[0].period, &end_two, phi_b) == CORE_OK);
        stm_multiply(phi_b, phi_a, phi_ab);

        double phi_direct[STM_SIZE];
        CHECK(propagate(&s0, orbit[0].period, &end_one, phi_direct) == CORE_OK);

        double worst = 0.0;
        for (int k = 0; k < STM_SIZE; k++) {
            double d = fabs(phi_ab[k] - phi_direct[k]);
            if (d > worst) {
                worst = d;
            }
        }
        /* Measured 8.7e-12 against entries reaching 12.7. */
        CHECK(worst < 1e-9);
    }

    /* stm_multiply and stm_identity on their own. */
    {
        double eye[STM_SIZE], a[STM_SIZE], c[STM_SIZE];
        stm_identity(eye);
        for (int k = 0; k < STM_SIZE; k++) {
            a[k] = (double)(k + 1);
        }
        stm_multiply(eye, a, c);
        for (int k = 0; k < STM_SIZE; k++) {
            CHECK_BITS_EQ(c[k], a[k]);
        }
        stm_multiply(a, eye, c);
        for (int k = 0; k < STM_SIZE; k++) {
            CHECK_BITS_EQ(c[k], a[k]);
        }
    }

    /* Argument checking. */
    {
        State end;
        double phi[STM_SIZE];
        Dop853Config cfg = tight();
        Dop853State st;
        memset(&st, 0, sizeof st);
        CHECK(stm_integrate(NULL, &ctx, &orbit[0].s, 1.0, &cfg, &st, &end, phi)
              == CORE_ERR_INVALID_ARG);

        Vec3d r[DOP853_MAX_BLOCKS], v[DOP853_MAX_BLOCKS];
        memset(&st, 0, sizeof st);
        CHECK(dop853_integrate_blocks(accel_cr3bp_var, &ctx,
                                      DOP853_MAX_BLOCKS + 1, 0.0, 1.0,
                                      r, v, &cfg, &st) == CORE_ERR_INVALID_ARG);
        CHECK(dop853_integrate_blocks(accel_cr3bp_var, &ctx, 0, 0.0, 1.0,
                                      r, v, &cfg, &st) == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
