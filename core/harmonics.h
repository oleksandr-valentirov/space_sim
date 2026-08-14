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
 * Coefficients are FULLY NORMALISED (4-pi) C_nm, S_nm since ROADMAP K5b,
 * where they used to be unnormalised. That is not a change of taste: the
 * unnormalised form carries factors that overflow a double on their own long
 * before the coefficients get a chance to be small - see HARMONICS_MAX_DEGREE
 * below. Real gravity models are published normalised for exactly this
 * reason, so this is also the form the data arrives in.
 *
 * Cited numbers are usually unnormalised (J2 = -C_20 is), and
 * harmonics_set_unnormalised converts them at the one place each is written.
 * Nothing multiplies coefficients in-line: a conversion done by hand at four
 * call sites is a conversion forgotten at the fifth.
 *
 * Everything here is +, -, *, / and sqrt: no trigonometry, so it stays in
 * the deterministic zone (PROJECT.md section 4) and `make check-libm` sees
 * nothing new. */

#ifndef CORE_HARMONICS_H
#define CORE_HARMONICS_H

#include "vec3.h"

/* Fifty since K5b, eight before it, and the difference is the whole reason
 * that step exists.
 *
 * The old ceiling was not a matter of array sizes. Measured by raising it
 * to 64 and evaluating: the acceleration went NaN from about degree 44,
 * because the unnormalised formulation carried quantities that overflow a
 * double on their own - Re^n past DBL_MAX at n = 50 for the Moon's 1738 km
 * radius, |(x+iy)|^m at m = 49 for a low lunar orbit. Neither depends on
 * the coefficients, so no choice of data avoided them.
 *
 * The normalised form has no such factor. It carries (Re/r)^n, which is
 * below one everywhere outside the reference sphere, direction cosines
 * bounded by one, and normalised A_nm which stay within a few orders of
 * unity - so the ceiling is now an array-size decision rather than an
 * arithmetic one. Fifty is what GRAIL's mascons need (ROADMAP K5); the
 * triangular arrays grow quadratically, and at fifty a HarmonicsField is
 * about 21 kB, which is why FieldCtx borrows one rather than copying it
 * (K5a, core/field.h). */
#define HARMONICS_MAX_DEGREE 50
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
    /* FULLY NORMALISED, triangular, index harmonics_index(n, m). Write them
     * through harmonics_set_unnormalised when the source is cited in the
     * usual unnormalised form. */
    double c[HARMONICS_MAX_COEFFS];
    double s[HARMONICS_MAX_COEFFS];
} HarmonicsField;

/* Index of (n, m) in c[]/s[], 0 <= m <= n <= HARMONICS_MAX_DEGREE. */
static inline int harmonics_index(int n, int m)
{
    return n * (n + 1) / 2 + m;
}

/* N_nm = sqrt( (n-m)! (2n+1) (2 - delta_0m) / (n+m)! ), the factor relating
 * the two conventions: A_norm = N_nm * A_unnorm, and therefore
 * C_norm = C_unnorm / N_nm, since only the product of the two is physical.
 *
 * Computed by two short recursions rather than from factorials, which is not
 * an optimisation: (n+m)! at n = m = 50 is 1e158, and the ratio that survives
 * it is 1e-78. Forming either factorial first throws the answer away.
 *
 * Only sqrt and arithmetic, so this stays inside the deterministic zone and
 * can be called from anywhere, including at asset load. */
double harmonics_normalisation(int n, int m);

/* Write one unnormalised pair into a field, converting it (ROADMAP K5b).
 *
 * Exists so that the conversion happens in one place rather than at every
 * site holding a cited number - the cooker's J2, the asset reader, four
 * tests. Out-of-range (n, m) is ignored rather than being an error: this is
 * a setter for constants known at the call site, and there is nobody to
 * report to. Degree is NOT raised as a side effect; the caller says how far
 * its field goes. */
void harmonics_set_unnormalised(HarmonicsField *field, int n, int m,
                                double c, double s);

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

/* Gradient of that acceleration - equivalently the Hessian of the
 * potential below - row-major 3x3 and symmetric to the bit (ROADMAP K8a).
 *
 *     g_out[i * 3 + j] = d a_i / d x_j
 *
 * This is what a state transition matrix needs to describe a vessel flying
 * through a non-spherical field. Without it, field.c had to refuse to
 * linearise such a field at all rather than hand back a matrix matching
 * some other trajectory (core/field.h).
 *
 * Traceless away from the source, since each term is a solid harmonic and
 * satisfies Laplace's equation - a property worth knowing because it makes
 * a free and quite sharp self-check. */
void harmonics_gradient(const HarmonicsField *field, Vec3d r, double mu,
                        double g_out[9]);

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
