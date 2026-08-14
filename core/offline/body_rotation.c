/* See body_rotation.h. Everything below is the mean pole/prime-meridian
 * model that file cites - no periodic terms. */

#include "body_rotation.h"

#include <math.h>
#include <string.h>

/* Not M_PI: POSIX, not guaranteed by -std=c11 - see cheb_fit.c. */
static const double ROT_PI = 3.14159265358979323846;
static const double DAY = 86400.0;
static const double CENTURY = 36525.0 * 86400.0;

/* A body's IAU mean orientation: two-term polynomials in T (centuries) for
 * the pole, three-term in d (days) for the prime meridian - enough for
 * every body pck00011.tpc gives a non-approximate-only model for, and
 * exactly the terms it publishes. Degrees, matching the source. */
typedef struct {
    const char *name;
    double ra0, ra1;     /* pole right ascension: ra0 + ra1*T */
    double dec0, dec1;   /* pole declination: dec0 + dec1*T */
    double pm0, pm1, pm2; /* prime meridian: pm0 + pm1*d + pm2*d^2 */
} IauModel;

/* Earth's pole drift, pck00011.tpc BODY399_POLE_RA, degrees per century.
 * Named because the prime-meridian rate below has to cancel it exactly and
 * two copies of the same number would eventually stop being the same. */
#define EARTH_POLE_RA1 (-0.641)

/* Earth's prime meridian, from the Earth Rotation Angle instead of from
 * pck00011.tpc's approximate expression (ROADMAP K3b).
 *
 * ERA is defined (IAU 2000 resolution B1.8, IERS Conventions 2010 eq. 5.15)
 * as a linear function of UT1:
 *
 *     ERA = 2*pi * (0.7790572732640 + 1.00273781191135448 * Tu)
 *
 * with Tu in UT1 days from J2000. The classical GMST constant of the same
 * epoch, 6h41m50.54841s = 100.4606184 degrees at 0h UT1, is the same number
 * half a turn away, which is the cross-check that this is not a digit
 * mis-copied from one source.
 *
 * Three conversions turn that into the pm0/pm1 of a 3-1-3 sequence, and
 * each is a place a sign can go wrong, so each is stated:
 *
 *   1. ERA is measured from the CIO in the inertial frame; W is measured
 *      from the node of the body equator on the reference equator, which
 *      the sequence puts at Omega = 90 + alpha0. Hence the -90.
 *   2. The node moves, because alpha0 does. W is measured from that moving
 *      node, so its rate must carry the node's rate back out for the
 *      meridian's INERTIAL rate to be ERA's - hence the -EARTH_POLE_RA1
 *      term. Getting this wrong costs 0.641 deg per century, which is why
 *      test_body_rotation.c checks the meridian a century apart and not
 *      only at the epoch.
 *   3. ERA's argument is UT1 and this module's is the ephemeris clock (TDB,
 *      to within the 1.7 ms of TDB-TT that no part of this matters to).
 *      TT - UT1 at J2000 is 32.184 (TT-TAI, by definition) + 32 (TAI-UTC,
 *      the leap second count through 2000) - 0.3554 (UT1-UTC at J2000.0,
 *      IERS EOP) = 63.8286 s, and a fixed offset in the argument is a fixed
 *      offset in the angle, so it folds into pm0.
 *
 * Held fixed rather than modelled, that last one is also the whole of what
 * this leaves out; body_rotation.h says how much and why nobody can supply
 * it. Computed here from the cited constants rather than written out as
 * 190.1939377471: a derived decimal is a number nobody can check, and this
 * file is compiled once on one machine, so folding it at compile time costs
 * nothing that the asset does not already spend on cos(). */
#define ERA_TURNS_0    0.7790572732640
#define ERA_TURNS_RATE 1.00273781191135448
#define EARTH_DELTA_T_S 63.8286

#define EARTH_PM0 (ERA_TURNS_0 * 360.0 - 90.0 \
                   - ERA_TURNS_RATE * 360.0 * (EARTH_DELTA_T_S / 86400.0))
#define EARTH_PM1 (ERA_TURNS_RATE * 360.0 - EARTH_POLE_RA1 / 36525.0)

/* What the replaced expression was worth, measured rather than quoted.
 *
 * pck00011.tpc gives W = 190.147 + 360.9856235*d, and K3a reported it as
 * 1129 arcsec from the truth at J2000. That number compared the model, whose
 * argument is the ephemeris clock, against ERA evaluated as if that clock
 * were UT1 - two errors added together, the model's own and the 63.8 s the
 * clocks differ by, which is 960 arcsec of Earth all on its own. On the
 * clock the expression is actually evaluated on, its error at J2000 is 169
 * arcsec, which is what the file's own "at least 150 arcseconds" caveat
 * says it should be.
 *
 * The phase was not the larger half anyway. Its rate is 833 arcsec per
 * century away from ERA's, so the same expression is off by 1004 arcsec at
 * J2100 and 1842 at J2200 - and a rate error is the one that matters for an
 * ephemeris asked to cover two hundred years, because it does not stay put. */

/* Poles from NAIF generic PCK pck00011.tpc, Earth's prime meridian from the
 * ERA above - see body_rotation.h for the citations and what each entry
 * leaves out. */
static const IauModel MODELS[] = {
    { "earth", 0.0, EARTH_POLE_RA1, 90.0, -0.557, EARTH_PM0, EARTH_PM1, 0.0 },
    { "moon", 269.9949, 0.0031, 66.5392, 0.0130, 38.3213, 13.17635815,
     -1.4e-12 },
};
#define N_MODELS (sizeof MODELS / sizeof MODELS[0])

static double deg2rad(double deg)
{
    return deg * (ROT_PI / 180.0);
}

static void mat3_rz(double theta, double m[9])
{
    double c = cos(theta);
    double s = sin(theta);
    /* Passive (frame) rotation by theta about z - PROJECT.md's inertial to
     * body-fixed sense, the same convention classical orbital mechanics
     * uses for R3 in building the perifocal frame. */
    m[0] = c;  m[1] = s;  m[2] = 0.0;
    m[3] = -s; m[4] = c;  m[5] = 0.0;
    m[6] = 0.0; m[7] = 0.0; m[8] = 1.0;
}

static void mat3_rx(double theta, double m[9])
{
    double c = cos(theta);
    double s = sin(theta);
    m[0] = 1.0; m[1] = 0.0; m[2] = 0.0;
    m[3] = 0.0; m[4] = c;   m[5] = s;
    m[6] = 0.0; m[7] = -s;  m[8] = c;
}

static void mat3_mul(const double a[9], const double b[9], double out[9])
{
    for (int i = 0; i < 3; i++) {
        for (int j = 0; j < 3; j++) {
            double sum = 0.0;
            for (int k = 0; k < 3; k++) {
                sum += a[i * 3 + k] * b[k * 3 + j];
            }
            out[i * 3 + j] = sum;
        }
    }
}

/* Standard robust matrix-to-quaternion conversion (Shepperd's method):
 * branches on the largest of the trace and the three diagonal entries so
 * the sqrt argument never gets small enough for the division after it to
 * lose precision. m is row-major, m[i*3+j]. */
static Quat mat3_to_quat(const double m[9])
{
    double tr = m[0] + m[4] + m[8];
    Quat q;

    if (tr > 0.0) {
        double s = sqrt(tr + 1.0) * 2.0;
        q.w = 0.25 * s;
        q.x = (m[7] - m[5]) / s;
        q.y = (m[2] - m[6]) / s;
        q.z = (m[3] - m[1]) / s;
    } else if (m[0] > m[4] && m[0] > m[8]) {
        double s = sqrt(1.0 + m[0] - m[4] - m[8]) * 2.0;
        q.w = (m[7] - m[5]) / s;
        q.x = 0.25 * s;
        q.y = (m[1] + m[3]) / s;
        q.z = (m[2] + m[6]) / s;
    } else if (m[4] > m[8]) {
        double s = sqrt(1.0 + m[4] - m[0] - m[8]) * 2.0;
        q.w = (m[2] - m[6]) / s;
        q.x = (m[1] + m[3]) / s;
        q.y = 0.25 * s;
        q.z = (m[5] + m[7]) / s;
    } else {
        double s = sqrt(1.0 + m[8] - m[0] - m[4]) * 2.0;
        q.w = (m[3] - m[1]) / s;
        q.x = (m[2] + m[6]) / s;
        q.y = (m[5] + m[7]) / s;
        q.z = 0.25 * s;
    }

    return q;
}

static const IauModel *model_of(const char *name)
{
    for (size_t i = 0; i < N_MODELS; i++) {
        if (strcmp(MODELS[i].name, name) == 0) {
            return &MODELS[i];
        }
    }
    return NULL;
}

int body_rotation_has_model(const char *name)
{
    return name != NULL && model_of(name) != NULL;
}

CoreResult body_rotation_of(const char *name, double t, Quat *out)
{
    if (name == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    const IauModel *model = model_of(name);
    if (model == NULL) {
        *out = quat_identity();
        return CORE_OK;
    }

    double century = t / CENTURY;
    double day = t / DAY;

    double alpha0 = model->ra0 + model->ra1 * century;
    double delta0 = model->dec0 + model->dec1 * century;
    double w = model->pm0 + model->pm1 * day + model->pm2 * day * day;

    /* Classical 3-1-3 sequence, inertial to body-fixed: node at
     * Omega = 90 + alpha0, inclination i = 90 - delta0, then the prime
     * meridian angle W about the body's own pole. Node and inclination
     * chosen so the body-fixed z axis, carried back to the inertial frame,
     * comes out exactly (cos delta0 cos alpha0, cos delta0 sin alpha0,
     * sin delta0) - the textbook pole-direction identity from Omega/i,
     * checked against this exact model in test_body_rotation.c rather than
     * trusted from the derivation alone (ROADMAP K3). */
    double omega = deg2rad(90.0 + alpha0);
    double inc = deg2rad(90.0 - delta0);
    double pm = deg2rad(w);

    double r_omega[9], r_inc[9], r_pm[9], tmp[9], r[9];
    mat3_rz(omega, r_omega);
    mat3_rx(inc, r_inc);
    mat3_rz(pm, r_pm);

    mat3_mul(r_inc, r_omega, tmp);  /* R1(i) * R3(Omega) */
    mat3_mul(r_pm, tmp, r);         /* R3(W) * R1(i) * R3(Omega) */

    /* r rotates inertial to body-fixed; quat.h's convention is the other
     * way, so the quaternion built from r is conjugated once here rather
     * than the caller having to remember to invert it. */
    *out = quat_conjugate(mat3_to_quat(r));
    return CORE_OK;
}
