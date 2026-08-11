/* Sanity checks on the JPL Horizons fixtures (ROADMAP B1).
 *
 * The point of these is not precision. It is to prove that the data means
 * what the loader thinks it means: right centre, right frame, right units,
 * right time scale. Every one of those is a silent, catastrophic failure
 * mode, and every one of them is caught here by a bound so wide that no
 * plausible model error could trip it.
 *
 * Run from the repository root; paths are relative to it. */

#include "refdata.h"
#include "test.h"

#include <math.h>

#define MAX_SAMPLES 256

#define AU_M 1.495978707e11   /* IAU 2012 definition, exact by definition */

static size_t load(const char *path, RefSample *buf)
{
    size_t n = 0;
    CoreResult r = refdata_load_vectors(path, buf, MAX_SAMPLES, &n);
    if (r != CORE_OK) {
        fprintf(stderr, "  load(%s) -> %s\n", path, core_result_str(r));
    }
    CHECK(r == CORE_OK);
    return n;
}

int main(void)
{
    static RefSample sun[MAX_SAMPLES], earth[MAX_SAMPLES], moon[MAX_SAMPLES];

    size_t n_sun   = load("data/horizons/vec_sun.csv", sun);
    size_t n_earth = load("data/horizons/vec_earth.csv", earth);
    size_t n_moon  = load("data/horizons/vec_moon.csv", moon);

    CHECK(n_sun == 122);
    CHECK(n_earth == n_sun);
    CHECK(n_moon == n_sun);

    /* Time base: first sample is exactly J2000.0, and the step is exactly the
     * 30 days that were requested. A wrong time scale (UTC instead of TDB,
     * say) shows up as an offset here rather than as a mysterious position
     * error sixty steps later. */
    CHECK_BITS_EQ(earth[0].jdtdb, REFDATA_JD_J2000);
    CHECK_BITS_EQ(earth[0].s.t, 0.0);
    CHECK_BITS_EQ(earth[1].s.t, 30.0 * REFDATA_SEC_PER_DAY);
    for (size_t i = 1; i < n_earth; i++) {
        CHECK(earth[i].s.t > earth[i - 1].s.t);
    }

    /* Geometry, in metres. Bounds are deliberately loose. */
    for (size_t i = 0; i < n_earth; i++) {
        double r_earth = vec3_norm(earth[i].s.r);
        CHECK(r_earth > 0.97 * AU_M && r_earth < 1.03 * AU_M);

        double v_earth = vec3_norm(earth[i].s.v);
        CHECK(v_earth > 28.0e3 && v_earth < 31.0e3);

        /* The Moon relative to the Earth: perigee to apogee is roughly
         * 356000 to 407000 km. If the centre were wrong, this would come out
         * near 1 AU instead. */
        double d_moon = vec3_distance(moon[i].s.r, earth[i].s.r);
        CHECK(d_moon > 3.5e8 && d_moon < 4.1e8);

        /* Measured over this fixture: 965.8 m/s at apogee to 1101.7 m/s at
         * perigee, which vis-viva confirms for those two distances. The
         * bounds are set outside that, not at a remembered "about 1 km/s". */
        double v_moon = vec3_norm(vec3_sub(moon[i].s.v, earth[i].s.v));
        CHECK(v_moon > 0.95e3 && v_moon < 1.15e3);

        /* The Sun wobbles around the barycentre by at most a couple of solar
         * radii. This is the check that the centre really is the solar system
         * barycentre and not the Sun itself, in which case it would be 0. */
        double r_sun = vec3_norm(sun[i].s.r);
        CHECK(r_sun > 1.0e8 && r_sun < 2.0e9);
    }

    /* Frame: ICRF equatorial, not ecliptic. The Earth's orbit is inclined
     * about 23.4 degrees to the equator, so z reaches roughly 0.4 AU over a
     * year. In ecliptic coordinates z would stay near zero. This single
     * assertion is the difference between a working ephemeris and one that is
     * quietly rotated by 23 degrees. */
    {
        double max_abs_z = 0.0;
        for (size_t i = 0; i < n_earth; i++) {
            double az = fabs(earth[i].s.r.z);
            if (az > max_abs_z) {
                max_abs_z = az;
            }
        }
        CHECK(max_abs_z > 0.3 * AU_M);
    }

    /* Gravitational parameters, converted to m^3/s^2. */
    {
        static RefGm gm[16];
        size_t n_gm = 0;
        CoreResult r = refdata_load_gm("data/horizons/gm.csv", gm, 16, &n_gm);
        CHECK(r == CORE_OK);
        CHECK(n_gm == 6);

        double gm_sun = refdata_gm_of(gm, n_gm, "sun");
        double gm_earth = refdata_gm_of(gm, n_gm, "earth");
        double gm_moon = refdata_gm_of(gm, n_gm, "moon");

        CHECK(gm_sun > 1.32e20 && gm_sun < 1.33e20);
        CHECK(gm_earth > 3.98e14 && gm_earth < 3.99e14);
        CHECK(gm_moon > 4.90e12 && gm_moon < 4.91e12);

        /* Known mass ratios, as an independent cross-check of the parsing:
         * the Sun is about 333000 Earths, the Earth about 81.3 Moons. If a
         * unit conversion were wrong these would be off by powers of a
         * thousand. */
        CHECK(gm_sun / gm_earth > 332000.0 && gm_sun / gm_earth < 334000.0);
        CHECK(gm_earth / gm_moon > 81.0 && gm_earth / gm_moon < 81.6);

        /* A missing body returns 0.0 rather than something plausible. */
        CHECK_BITS_EQ(refdata_gm_of(gm, n_gm, "pluto"), 0.0);
    }

    /* Buffer discipline: too small a buffer is reported, not overrun. */
    {
        RefSample tiny[4];
        size_t n = 0;
        CoreResult r = refdata_load_vectors("data/horizons/vec_earth.csv",
                                            tiny, 4, &n);
        CHECK(r == CORE_ERR_BUFFER_TOO_SMALL);
        CHECK(n == 4);
    }
    {
        RefSample buf[MAX_SAMPLES];
        size_t n = 0;
        CoreResult r = refdata_load_vectors("data/horizons/nope.csv",
                                            buf, MAX_SAMPLES, &n);
        CHECK(r == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
