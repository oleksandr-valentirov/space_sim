/* Atmospheric density and drag (ROADMAP K7a).
 *
 * The last of the four effects PROJECT.md section 4 says a point mass cannot
 * produce, and the one that gives entry, aerobraking and low-orbit decay their
 * physics. This file is the isolated math, in the same split K1 used before
 * K2, K3a before K3b and K6a before K6b: a table and a force here, bodies and
 * the asset in K7b. Nothing here knows what an ephemeris is.
 *
 * ---
 *
 * DENSITY WITHOUT exp(), and this is what shapes the file.
 *
 * Density falls about fourteen orders of magnitude from sea level to 1000 km.
 * That rules out the two obvious representations immediately. A table of
 * densities interpolated linearly is wrong between its own nodes in the way
 * that matters here - relatively - because a straight line between two values
 * a decade apart misses the middle by a factor of two. Interpolating log
 * density fixes that and then needs exp() to get back, and CLAUDE.md invariant
 * 3 allows +, -, *, / and sqrt and nothing else: libm's exp is not guaranteed
 * bit-identical between platforms or even between libc versions.
 *
 * So the model is layered exponential - the standard astrodynamics form - and
 * exp itself is ours. atmosphere_exp_neg below is the Taylor series with range
 * reduction by halving, and its error is MEASURED against libm in
 * core/test/test_atmosphere.c rather than quoted. That is the same bargain
 * core/srp.h struck for the arc cosine of the penumbra: the real function,
 * evaluated with an approximation whose error is a number we know.
 *
 * ---
 *
 * DRAG IS THE FIRST FORCE THAT READS THE VELOCITY, and that has consequences
 * beyond this file worth stating where they will be found.
 *
 * PROJECT.md section 4 chose DOP853 over RKN precisely because drag breaks
 * y'' = f(t, y). Here that becomes concrete, and so does the shape of the
 * Jacobian:
 *
 *   - d(a)/d(v) is symmetric. It is s*(|v| I + v v^T / |v|), and both pieces
 *     are.
 *   - d(a)/d(r) IS NOT. It is a rank-one outer product of v with the local
 *     vertical, because position enters only through the density. Every force
 *     in the core before this one had a symmetric position gradient, and
 *     core/field.h promises callers one; K7b is where that promise has to
 *     narrow to "symmetric while drag is off".
 *
 * Neither Jacobian here includes the wind. A rotating atmosphere makes v_rel
 * depend on position, so the true d(a)/d(r) carries a second term through
 * d(v_rel)/d(r) = -[omega]x. Composing that is core/field.c's job in K7b: it
 * is the file that knows a body's rotation, and it has d(a)/d(v) from here to
 * compose with. Splitting it that way keeps this file free of any notion of a
 * body, which is the whole point of the K7a/K7b division. */

#ifndef CORE_ATMOSPHERE_H
#define CORE_ATMOSPHERE_H

#include "vec3.h"

/* Enough for the reference model's 28 bands with room to spare. A cap rather
 * than an allocation because C does not allocate buffers with data in it
 * (PROJECT.md section 5), and because a model needing hundreds of layers would
 * be a different representation, not a bigger array. */
#define ATMOSPHERE_MAX_LAYERS 32

/* Terms of the Taylor series for exp, and how many halvings the range
 * reduction is allowed. Both are named because the test reads them: the first
 * decides the error it measures, the second decides where the model stops.
 *
 * Six halvings puts the ceiling at x = 64, and exp(-64) is 1.6e-28. Multiplied
 * by the densest air in the table that is 2e-28 kg/m^3, which is not a small
 * density but no density at all - the drag it produces is around 1e-22 m/s^2.
 * So the cutoff sits where the VALUE is zero, not where patience runs out,
 * and the discontinuity it introduces is below anything double precision can
 * carry into an answer. */
#define ATMOSPHERE_EXP_TERMS    17
#define ATMOSPHERE_EXP_HALVINGS 6

typedef struct {
    /* Altitude above the body's reference surface at which this band starts,
     * metres. Bands must be sorted ascending; the last one extends upward
     * until atmosphere_exp_neg underflows. */
    double base_altitude_m;

    /* Density at base_altitude_m, kg/m^3. */
    double base_density;

    /* e-folding height of this band, metres. Non-positive means the band is
     * unusable and is treated as vacuum, rather than as a division that
     * produces infinity. */
    double scale_height_m;
} AtmosphereLayer;

typedef struct {
    int             n_layers;   /* 0 means the body has no atmosphere */
    AtmosphereLayer layer[ATMOSPHERE_MAX_LAYERS];
} AtmosphereModel;

/* The Earth, US Standard Atmosphere 1976 in the layered form Vallado
 * tabulates (Fundamentals of Astrodynamics and Applications, table 8-4,
 * "Exponential Atmospheric Model"), 0 to 1000 km in 28 bands.
 *
 * Quoted, not invented - the same rule data/horizons/README.md states for
 * every other number the core relies on. It is also self-checking, and
 * core/test/test_atmosphere.c uses that: each band's base density is what the
 * band below it predicts at that altitude, so a single mistyped digit breaks
 * continuity somewhere. That is an oracle which does not consist of quoting
 * the same table twice.
 *
 * WHAT THIS MODEL DOES NOT HAVE is solar activity. Density above roughly
 * 300 km swings by more than an order of magnitude over the eleven-year cycle,
 * and this table is a single static profile. That is deliberate for now: the
 * multiplier which would carry it (ROADMAP K7) is plumbed through the field
 * and the save from the start and left at 1.0, because nobody can say where
 * the next solar maximum falls - the same class of unknowable as the drift of
 * TT - UT1 in K3b - so it belongs to a game-side model, not to asset data. */
extern const AtmosphereModel ATMOSPHERE_EARTH_USSA76;

/* exp(-x), ours, for any x. Accurate to a few times 1e-15 relative over the
 * range that matters and exactly 0 above x = 64; see the constants above.
 *
 * Public for the same reason srp_acos is: an approximation whose error nobody
 * measures is a guess. core/test/test_atmosphere.c sweeps it against libm and
 * fails if the error moves. That test may link libm; the runtime may not. */
double atmosphere_exp_neg(double x);

/* Density at a geometric altitude above the body's reference surface, and its
 * vertical derivative. Either output pointer may be NULL.
 *
 * Below the first band the density is held at that band's base value rather
 * than extrapolated downward. A vessel there is inside the ground, and the
 * exponential run backwards would reach infinity before it reached the centre;
 * a large finite density kills the trajectory loudly, which is the outcome
 * least likely to be mistaken for a working orbit (the same choice
 * core/srp.c makes for a vessel inside a body).
 *
 * A model with no layers is vacuum: zero density, zero derivative, and the
 * trajectory is bit-for-bit what it was before drag existed. */
void atmosphere_density(const AtmosphereModel *m, double altitude_m,
                        double *rho_out, double *drho_dh_out);

/* Drag acceleration, opposing the velocity relative to the air:
 *
 *     a = -1/2 * rho * (cd*A/m) * |v_rel| * v_rel
 *
 * coeff is cd*A/m in m^2/kg - one number rather than three, exactly as
 * core/srp.h keeps Cr*A/m, and for the same reason: the model depends only on
 * the product, and carrying the factors apart would suggest otherwise.
 *
 * Note the two powers of speed. This is the term that makes acceleration
 * depend on velocity and therefore the term RKN could not have carried. */
void drag_accel(double density, double coeff, Vec3d v_rel, Vec3d *a_out);

/* Both Jacobians of that acceleration, row-major 3x3.
 *
 *   dadv = s * ( |v| I + v v^T / |v| ),          s = -1/2 rho coeff
 *   dadr = -1/2 coeff |v| (drho_dh) * v up^T
 *
 * `up` is the unit vector along increasing altitude - the local vertical -
 * because position reaches the force only through the density. dadr is
 * therefore rank one and NOT symmetric; see the header comment.
 *
 * Either output may be NULL. Neither includes the wind term; core/field.c
 * composes that in K7b, which is where omega lives. */
void drag_jacobian(double density, double drho_dh, double coeff,
                   Vec3d v_rel, Vec3d up, double dadr[9], double dadv[9]);

#endif /* CORE_ATMOSPHERE_H */
