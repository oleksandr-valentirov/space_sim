/* State transition matrix by variational equations (ROADMAP C2b).
 *
 * Phi(t) is the derivative of the state at time t with respect to the state at
 * t0. It answers the only question differential correction ever asks - "move
 * the start by this much and where does the end go" - and it answers it
 * without finite differences, which at the accuracies involved here would be
 * dominated by their own subtraction error.
 *
 * The same matrix is what the uncertainty machinery needs later (PROJECT.md
 * section 8) to push a covariance forward, and what the monodromy matrix of
 * ROADMAP C3 is: Phi over one full period.
 *
 * Layout is row-major and stays that way at the FFI boundary, matching
 * prop_run_stm in PROJECT.md section 5:
 *
 *     phi[i * 6 + j] = d y_i(t) / d y_j(t0)
 *
 * with the state ordered (x, y, z, vx, vy, vz).
 *
 * Note what this does not contain: any dynamics. The caller supplies a
 * BlockAccelFunc that knows both its own acceleration and its Jacobian - see
 * accel_cr3bp_var. Everything here is bookkeeping around it. */

#ifndef CORE_STM_H
#define CORE_STM_H

#include "integrator.h"

/* Six columns of the STM plus the reference trajectory. Equals
 * DOP853_MAX_BLOCKS, and that is not a coincidence: the STM is why blocks
 * exist. */
#define STM_BLOCKS 7

/* Number of doubles in a 6x6 matrix. */
#define STM_SIZE 36

/* Integrate the state and its transition matrix from in->t to t_end.
 *
 * phi is written on success and untouched on failure. Passing NULL for phi is
 * allowed and makes this an ordinary propagation - useful when a caller wants
 * the same code path with and without sensitivities.
 *
 * io behaves as in dop853_integrate: the step carries between calls. */
CoreResult stm_integrate(BlockAccelFunc f, void *ctx, const State *in,
                         double t_end, const Dop853Config *cfg,
                         Dop853State *io, State *out, double phi[STM_SIZE]);

/* c = a * b, all 6x6 row-major. c may not alias a or b.
 *
 * Present because transition matrices compose - Phi(t0->t2) = Phi(t1->t2) *
 * Phi(t0->t1) - and multiple shooting in C4 will lean on that constantly. */
void stm_multiply(const double a[STM_SIZE], const double b[STM_SIZE],
                  double c[STM_SIZE]);

void stm_identity(double phi[STM_SIZE]);

/* How far Phi is from being symplectic: max |(Phi^T J Phi - J)_ij|.
 *
 * The CR3BP is Hamiltonian, so the true Phi satisfies Phi^T J Phi = J exactly,
 * for every trajectory and every duration. That makes this the cheapest
 * available check on a computed STM - it needs no reference orbit, no second
 * implementation and no finite differences, and it fails loudly on a sign
 * error in the Jacobian, which is the mistake this code is most likely to
 * contain. */
double stm_symplectic_defect(const double phi[STM_SIZE]);

/* ---- Stability of a periodic orbit (ROADMAP C3) ------------------------- */

typedef struct {
    /* The two nontrivial values of lambda + 1/lambda. When real_pair is 0
     * these are instead the real and imaginary parts of one conjugate pair. */
    double invariant[2];
    int    real_pair;

    /* |invariant| / 2, which is the stability index the JPL catalogue
     * publishes. Meaningful only when real_pair is 1. */
    double index[2];

    /* Modulus of the largest eigenvalue: the factor by which a perturbation
     * is multiplied per revolution. This is the number ROADMAP C3 asks for,
     * and it is NOT the stability index - for a real pair the index is
     * (lambda + 1/lambda)/2, which for lambda = 594 is 297. Reporting the
     * index as a growth rate understates it by a factor of two, and for
     * weakly unstable orbits by much more: index 1.015 means lambda 1.19. */
    double lambda_max;

    /* How far mu = 2 fails to be a root of the reduced cubic.
     *
     * Every periodic orbit of an autonomous Hamiltonian system has a pair of
     * unit eigenvalues - one from translation along the orbit, one from the
     * neighbouring orbit at the same energy. The factorisation below assumes
     * it. This residual is what the assumption costs, so it doubles as an
     * accuracy measure of the monodromy matrix itself: measured 5e-10 for a
     * well-integrated orbit and 2e-2 for the near-rectilinear member of the
     * family whose integration is known to be poor. */
    double unit_pair_residual;
} StmStability;

/* Eigenvalue moduli of a monodromy matrix, without an eigenvalue solver.
 *
 * The matrix is similar to a symplectic one, so its characteristic polynomial
 * is reciprocal: eigenvalues come in pairs lambda, 1/lambda. Substituting
 * mu = lambda + 1/lambda turns the sixth-degree polynomial into a cubic,
 * dividing out the known unit pair leaves a quadratic, and the quadratic is
 * solved in closed form. The coefficients come from the traces of m, m^2 and
 * m^3 through Newton's identities.
 *
 * That matters beyond elegance: QR iteration would be several hundred lines
 * and would need libm, while this needs +, -, *, / and sqrt, so it runs in
 * the deterministic zone.
 *
 * Note the matrix may be given in velocity coordinates - similarity preserves
 * the characteristic polynomial, so no canonical transformation is needed
 * here, unlike stm_symplectic_defect. */
CoreResult stm_monodromy_stability(const double m[STM_SIZE],
                                   StmStability *out);

#endif /* CORE_STM_H */
