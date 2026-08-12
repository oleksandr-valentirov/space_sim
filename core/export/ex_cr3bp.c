/* Export: the CR3BP model, as pictures (Milestone 0 delivery).
 *
 * Three files, and each answers a question the unit tests answer only as a
 * pass:
 *
 *   cr3bp_points.csv   where the primaries and the five Lagrange points are
 *   halo_family.csv    the five catalogue halo orbits, sampled along a period
 *   stability.csv      how fast a perturbation grows, against lambda^n
 *
 * The third is the interesting one. core/test/test_stability.c checks that the
 * measured growth agrees with the monodromy eigenvalue, and separately records
 * that orbit 0 does NOT grow at its eigenvalue at all - its unit eigenvalue
 * pair is defective, so a displacement drifts along the family linearly in
 * time and swamps a factor of 1.19 for many revolutions. That is a sentence in
 * a comment; plotted next to its prediction it is obvious at a glance, which
 * is the difference this delivery is for.
 *
 * Trajectories are advanced leg by leg with the step carried across, rather
 * than re-propagated from the start for each sample. That is the same
 * continuation the runtime uses across a save (integrator.h), it is far
 * cheaper, and the leg boundaries cost well under the tolerance.
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "csv.h"
#include "refdata.h"
#include "stm.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16
#define SAMPLES_PER_PERIOD 360

/* The seed displacement, small enough that even after the growth below the
 * motion is still linear. At 1e-10 the last revolution of orbit 1151 has
 * already left the linear regime (test_stability.c). */
#define SEED 1e-12

/* Stop a growth curve once it leaves the linear regime; the prediction it is
 * being compared against is a linear one. */
#define SEED_LIMIT 1e-2
#define MAX_REVOLUTIONS 12

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

/* Advance `s` to time t_end, continuing from `st`. */
static int advance(State *s, double t_end, Dop853State *st)
{
    Dop853Config cfg = tight();
    State out;

    if (dop853_integrate(accel_cr3bp, &ctx, s, t_end, &cfg, st, &out)
        != CORE_OK) {
        return 0;
    }
    *s = out;
    return 1;
}

static int export_points(void)
{
    Csv c;
    if (!csv_open(&c, "build/csv/cr3bp_points.csv", "name,x,y,z")) {
        return 0;
    }

    /* The normalisation, restated where the plot can see it: the primary of
     * mass 1-mu sits at (-mu, 0, 0) and the secondary at (1-mu, 0, 0). */
    csv_named(&c, "earth", 3, -mu, 0.0, 0.0);
    csv_named(&c, "moon", 3, 1.0 - mu, 0.0, 0.0);

    for (int p = 1; p <= 5; p++) {
        Vec3d l;
        if (cr3bp_lagrange(mu, p, &l) != CORE_OK) {
            return 0;
        }

        char name[8];
        snprintf(name, sizeof name, "l%d", p);
        csv_named(&c, name, 3, l.x, l.y, l.z);
    }

    return csv_close(&c);
}

static int export_family(void)
{
    Csv c;
    if (!csv_open(&c, "build/csv/halo_family.csv",
                  "orbit,t,x,y,z,vx,vy,vz,jacobi")) {
        return 0;
    }

    for (size_t i = 0; i < n_orbits; i++) {
        State s = orbit[i].s;
        s.t = 0.0;

        Dop853State st;
        memset(&st, 0, sizeof st);

        for (int k = 0; k <= SAMPLES_PER_PERIOD; k++) {
            double t = orbit[i].period * (double)k
                       / (double)SAMPLES_PER_PERIOD;
            if (k > 0 && !advance(&s, t, &st)) {
                return 0;
            }

            csv_row(&c, 9, (double)orbit[i].index, s.t,
                    s.r.x, s.r.y, s.r.z, s.v.x, s.v.y, s.v.z,
                    cr3bp_jacobi(s.r, s.v, mu));
        }
    }

    return csv_close(&c);
}

/* Distance in the full six-dimensional state between a trajectory and a
 * displaced copy of it, as test_stability.c defines it. */
static double separation(const State *a, const State *b)
{
    return sqrt(vec3_norm_sq(vec3_sub(b->r, a->r))
                + vec3_norm_sq(vec3_sub(b->v, a->v)));
}

static int export_stability(void)
{
    /* `envelope` is sep0 * lambda^n, and it predicts the SLOPE, not the
     * magnitude. The seed is a generic displacement, so only part of it lies
     * along the unstable eigenvector; the rest decays. For orbit 767 the curve
     * therefore sits a constant factor 3.7 below the envelope while running
     * exactly parallel to it, and it is the parallelism that says the growth
     * is the eigenvalue's. */
    Csv c;
    if (!csv_open(&c, "build/csv/stability.csv",
                  "orbit,lambda,revolution,separation,envelope")) {
        return 0;
    }

    for (size_t i = 0; i < n_orbits; i++) {
        Dop853Config cfg = tight();
        Dop853State mono_st;
        memset(&mono_st, 0, sizeof mono_st);

        double m[STM_SIZE];
        State end;
        if (stm_integrate(accel_cr3bp_var, &ctx, &orbit[i].s,
                          orbit[i].period, &cfg, &mono_st, &end, m)
            != CORE_OK) {
            return 0;
        }

        StmStability stab;
        if (stm_monodromy_stability(m, &stab) != CORE_OK) {
            return 0;
        }

        State ref = orbit[i].s;
        State displaced = orbit[i].s;
        displaced.r.x += SEED;
        displaced.r.z += SEED;
        displaced.v.y += SEED;

        double sep0 = separation(&ref, &displaced);

        Dop853State ref_st, disp_st;
        memset(&ref_st, 0, sizeof ref_st);
        memset(&disp_st, 0, sizeof disp_st);

        for (int rev = 0; rev <= MAX_REVOLUTIONS; rev++) {
            double t = orbit[i].period * (double)rev;
            if (rev > 0
                && (!advance(&ref, t, &ref_st)
                    || !advance(&displaced, t, &disp_st))) {
                return 0;
            }

            double sep = separation(&ref, &displaced);

            /* pow() is libm, and this program may use it - it is a diagnostic
             * driver, not the runtime (csv.h). Repeated multiplication would
             * do as well; this is clearer. */
            double envelope = sep0 * pow(stab.lambda_max, (double)rev);

            csv_row(&c, 5, (double)orbit[i].index, stab.lambda_max,
                    (double)rev, sep, envelope);

            if (sep > SEED_LIMIT) {
                break;
            }
        }
    }

    return csv_close(&c);
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit,
                          MAX_ORBITS, &n_orbits) != CORE_OK
        || refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK) {
        fprintf(stderr, "ex_cr3bp: cannot read data/jpl_halo/\n");
        fprintf(stderr, "  run from the repository root\n");
        return 1;
    }

    ctx.mu = mu;

    printf("ex_cr3bp: %zu catalogue orbits, mu = %.17g\n", n_orbits, mu);

    if (!export_points() || !export_family() || !export_stability()) {
        return 1;
    }

    return 0;
}
