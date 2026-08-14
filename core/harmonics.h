/* Spherical harmonic gravity by Pines' recursion (ROADMAP K1).
 *
 * A point mass is not enough for half of PROJECT.md section 4's geopotential
 * table: J2 precession, lunar mascon instability, none of it exists on a
 * sphere. This is the shared math both the ephemeris cooker (J2 of Earth and
 * the Moon, K2) and the vessel force model (K4) will call - degree and order
 * are parameters, not baked in, so the same code serves a bare J2 term now
 * and a degree-50 GRAIL field later (K5).
 *
 * Pines (1973), not Cunningham: Cunningham's recursion divides by
 * sqrt(x^2+y^2), which is exactly zero on the rotation axis, and PROJECT.md
 * section 4 requires no pole singularity. Pines works instead through the
 * direction cosines s=x/r, t=y/r, u=z/r, none of which is ever divided by,
 * only multiplied.
 *
 * Coefficients are unnormalised C_nm, S_nm - the form the closed-form
 * derivation below and the recursion both use directly. Real gravity models
 * (GRAIL, EGM) publish 4-pi normalised coefficients; converting them is a
 * K5 concern, done once offline when that data is imported, not here.
 *
 * Everything here is +, -, *, / and sqrt: no trigonometry, so it stays in
 * the deterministic zone (PROJECT.md section 4) and `make check-libm` sees
 * nothing new. */

#ifndef CORE_HARMONICS_H
#define CORE_HARMONICS_H

#include "vec3.h"

/* Raised only when K5 actually needs a higher degree (GRAIL, ~50). Every
 * triangular array below is sized off this, so raising it is a one-line
 * change, not a rewrite - but it also grows every one of those arrays
 * quadratically, so it is deliberately not raised speculatively. */
#define HARMONICS_MAX_DEGREE 8
#define HARMONICS_MAX_COEFFS \
    ((HARMONICS_MAX_DEGREE + 1) * (HARMONICS_MAX_DEGREE + 2) / 2)

/* A body's field in its own body-fixed frame. Degree and order share one
 * bound (no rectangular truncation) because nothing downstream needs one.
 * Terms below degree 2 are the caller's job: a body-fixed frame centred on
 * the body's own centre of mass has C_00 = S_11 = C_11 = 0 by construction
 * (degree 0 is the point mass, already handled elsewhere - see field.h -
 * and degree 1 vanishes for that frame choice), so this module only ever
 * sums degree 2 and up. */
typedef struct {
    int    degree;                       /* highest n present; < 2 disables the field */
    double re;                           /* reference radius, metres */
    double c[HARMONICS_MAX_COEFFS];      /* triangular, index harmonics_index(n, m) */
    double s[HARMONICS_MAX_COEFFS];
} HarmonicsField;

/* Index of (n, m) in c[]/s[], 0 <= m <= n <= HARMONICS_MAX_DEGREE. */
static inline int harmonics_index(int n, int m)
{
    return n * (n + 1) / 2 + m;
}

/* Acceleration at r (metres, body-fixed frame) from every term of degree 2
 * and up. mu is the body's own gravitational parameter - the same one the
 * point-mass term already uses, kept separate from HarmonicsField because
 * every caller already has it from the ephemeris.
 *
 * Derived directly from harmonics_potential by differentiating the same sum
 * term by term (see ROADMAP K1); checked there against the closed-form J2
 * acceleration rather than trusted on inspection. */
void harmonics_accel(const HarmonicsField *field, Vec3d r, double mu,
                     Vec3d *a_out);

/* The potential U such that harmonics_accel computes +grad(U) (PROJECT.md
 * section 4's sign convention: U = mu/r + perturbations, acceleration is the
 * plain gradient, not its negative - matching accel_field's point-mass term
 * with a body at the origin).
 *
 * Exists for two reasons: core/test/test_harmonics.c finite-differences it
 * against harmonics_accel as the general (n, m) check the closed-form J2
 * comparison cannot be, the same role stm_symplectic_defect plays for the
 * STM; and it is the natural energy diagnostic once vessels fly through a
 * real field (mirrors nbody_energy). Not on the hot path today. */
void harmonics_potential(const HarmonicsField *field, Vec3d r, double mu,
                         double *u_out);

#endif /* CORE_HARMONICS_H */
