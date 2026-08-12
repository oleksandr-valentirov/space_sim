/* The instantaneous synodic frame of a real pair of bodies (ROADMAP C4).
 *
 * The CR3BP lives in a frame that rotates uniformly with two primaries a fixed
 * distance apart. The real Earth and Moon do neither: their separation varies
 * by a tenth over a month and their angular rate varies with it. So a halo
 * orbit found in the CR3BP is not a trajectory in the real system, it is a
 * shape in a frame that does not quite exist.
 *
 * This builds the frame that does exist - rebuilt at every instant from where
 * the two bodies actually are - and converts states in and out of it. That
 * gives two things C4 needs: a way to turn a CR3BP orbit into a starting guess
 * for the real model, and a way to ask "is the vessel still near L2" of a
 * trajectory that has no L2 in it.
 *
 * Definition, at time t, for bodies P (primary) and S (secondary):
 *
 *   d = R_S - R_P,   L = |d|,   x = d / L
 *   h = d x d_dot,   z = h / |h|,   y = z cross x
 *   origin = the (mu-weighted) barycentre of P and S
 *   omega  = h / L^2
 *
 * Lengths are scaled by L and times by 1/|omega|, so a state that was
 * dimensionless in the CR3BP comes out in metres and metres per second, and
 * the secondary sits at x = 1 - mu by construction.
 *
 * ---
 *
 * One approximation, stated because it is the kind that is invisible later.
 * omega = h / L^2 reproduces dx/dt exactly, and gives dz/dt = 0 - that is, it
 * treats the orbit plane as fixed. The real Earth-Moon plane precesses, by
 * about 5 degrees of inclination over 18.6 years, so this omits a rotation of
 * some 1e-8 rad/s. The alternative needs the accelerations of both bodies to
 * differentiate h, which drags the force model into a coordinate
 * transformation; the error is measured in core/test/test_frame.c instead, as
 * the residual velocity of the Moon in its own frame. */

#ifndef CORE_FRAME_H
#define CORE_FRAME_H

#include "ephemeris.h"
#include "vec3.h"

typedef struct {
    Vec3d origin;        /* barycentre of the pair, inertial, metres */
    Vec3d origin_rate;   /* and its velocity */

    Vec3d x, y, z;       /* orthonormal basis, inertial components */
    Vec3d omega;         /* angular velocity of the basis, rad/s */

    double length;       /* L, metres: the distance between the bodies */
    double length_rate;  /* dL/dt, m/s: zero in the CR3BP, not here */
    double rate;         /* |omega|, rad/s: one dimensionless time unit */

    double mu;           /* mu_S / (mu_P + mu_S) */
    double t;            /* the epoch this frame was built for */
} SynodicFrame;

/* Build the frame from the ephemeris at time t. */
CoreResult frame_synodic(const EphemerisCtx *eph, int primary, int secondary,
                         double t, SynodicFrame *out);

/* Dimensionless CR3BP state -> inertial metres and m/s at the frame's epoch.
 * in->t is ignored; out->t is set to the frame's epoch. */
void frame_to_inertial(const SynodicFrame *f, const State *in, State *out);

/* And back. Exact inverse of frame_to_inertial for the same frame. */
void frame_from_inertial(const SynodicFrame *f, const State *in, State *out);

#endif /* CORE_FRAME_H */
