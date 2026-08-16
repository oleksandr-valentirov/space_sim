#!/usr/bin/env python3
"""Generate core/dop853_coeffs.h from the vendored SciPy source.

Generated rather than typed by hand: there are about eighty coefficients at 27
significant digits. One wrong digit need not break the order of the method,
only the error constant -- the order test would still pass while the
integrator quietly did more work. Mechanical transcription rules that out.

The generated header is committed, so the build needs no Python. Rerun only
when the source changes.

    python3 scripts/gen_dop853_coeffs.py

No numpy needed: the shim below supports exactly the operations the vendored
file uses.
"""

import sys
from pathlib import Path

SRC = Path("data/vendor/dop853_coefficients_scipy.py")
DST = Path("core/dop853_coeffs.h")


class Arr:
    """Minimal array: indexing, slices, two-dimensional access."""

    def __init__(self, data):
        self.data = data

    def __getitem__(self, key):
        if isinstance(key, tuple):
            i, j = key
            row = self.data[i]
            return Arr(list(row[j])) if isinstance(j, slice) else row[j]
        if isinstance(key, slice):
            return Arr(list(self.data[key]))
        value = self.data[key]
        return Arr(list(value)) if isinstance(value, list) else value

    def __setitem__(self, key, value):
        raw = value.data if isinstance(value, Arr) else value
        if isinstance(key, tuple):
            i, j = key
            if isinstance(j, slice):
                self.data[i][j] = list(raw)
            else:
                self.data[i][j] = raw
        elif isinstance(key, slice):
            self.data[key] = list(raw)
        else:
            self.data[key] = raw

    def copy(self):
        return Arr(list(self.data))

    def __len__(self):
        return len(self.data)


class NumpyShim:
    @staticmethod
    def array(values):
        return Arr(list(values))

    @staticmethod
    def zeros(shape):
        if isinstance(shape, tuple):
            rows, cols = shape
            return Arr([[0.0] * cols for _ in range(rows)])
        return Arr([0.0] * shape)


def fmt(x):
    """17 significant digits -- exactly what a double round-trip needs."""
    return repr(float(x)) if len(repr(float(x))) <= 24 else "%.17g" % float(x)


def main():
    if not SRC.exists():
        sys.exit("missing %s" % SRC)

    # Drop `import numpy`: the shim is already in the namespace, and real numpy
    # may be absent -- not worth installing for eighty constants.
    source = "\n".join(
        line for line in SRC.read_text().splitlines()
        if line.strip() != "import numpy as np"
    )

    namespace = {"np": NumpyShim()}
    exec(compile(source, str(SRC), "exec"), namespace)

    n = namespace["N_STAGES"]
    c = namespace["C"]
    a = namespace["A"]
    b = namespace["B"]
    e3 = namespace["E3"]
    e5 = namespace["E5"]

    out = []
    w = out.append

    w("/* GENERATED FILE - do not edit.")
    w(" *")
    w(" * Regenerate with:  python3 scripts/gen_dop853_coeffs.py")
    w(" * Source: data/vendor/dop853_coefficients_scipy.py (SciPy, BSD-3-Clause;")
    w(" * coefficients originally from Hairer, Norsett and Wanner).")
    w(" *")
    w(" * Dormand-Prince 8(5,3): an eighth order solution with fifth and third")
    w(" * order embedded estimators. Only the first %d stages are emitted; the" % n)
    w(" * extended stages in the source exist for dense output, which the")
    w(" * integrator does not use yet.")
    w(" *")
    w(" * Transcribed mechanically rather than by hand: a single wrong digit in")
    w(" * one of these need not break the order of the method, only the error")
    w(" * constant, so the order test would still pass while the integrator")
    w(" * quietly did more work than necessary.")
    w(" */")
    w("")
    w("#ifndef CORE_DOP853_COEFFS_H")
    w("#define CORE_DOP853_COEFFS_H")
    w("")
    w("#define DOP853_STAGES %d" % n)
    w("")

    w("/* Stage times, as fractions of the step. */")
    w("static const double DOP853_C[DOP853_STAGES] = {")
    for i in range(n):
        w("    %s," % fmt(c[i]))
    w("};")
    w("")

    w("/* Stage coefficients, lower triangular. */")
    w("static const double DOP853_A[DOP853_STAGES][DOP853_STAGES] = {")
    for i in range(n):
        row = ", ".join(fmt(a[i, j]) for j in range(n))
        w("    { %s }," % row)
    w("};")
    w("")

    w("/* Weights of the eighth order solution. */")
    w("static const double DOP853_B[DOP853_STAGES] = {")
    for i in range(n):
        w("    %s," % fmt(b[i]))
    w("};")
    w("")

    w("/* Error estimators. Length is STAGES+1: the last entry weights the")
    w(" * derivative at the end of the step, which is evaluated anyway for the")
    w(" * next step (first-same-as-last). */")
    w("static const double DOP853_E3[DOP853_STAGES + 1] = {")
    for i in range(n + 1):
        w("    %s," % fmt(e3[i]))
    w("};")
    w("")
    w("static const double DOP853_E5[DOP853_STAGES + 1] = {")
    for i in range(n + 1):
        w("    %s," % fmt(e5[i]))
    w("};")
    w("")
    w("#endif /* CORE_DOP853_COEFFS_H */")

    DST.write_text("\n".join(out) + "\n")

    # Consistency check here, so a corrupt header never reaches the build.
    worst_row = 0.0
    for i in range(n):
        row_sum = sum(a[i, j] for j in range(n))
        worst_row = max(worst_row, abs(row_sum - c[i]))
    b_sum = sum(b[i] for i in range(n))

    print("wrote %s" % DST)
    print("  max |sum(A[i]) - C[i]| = %.3e" % worst_row)
    print("  |sum(B) - 1|           = %.3e" % abs(b_sum - 1.0))
    # Threshold 1e-14, not 1e-15: a row of 12 terms including ~0.9 and -0.88
    # sums with about 8 ULP of error -- measured 1.78e-15. Costs no
    # sensitivity: one corrupt digit even at the tenth place shifts the sum by
    # 1e-10 and gets caught.
    if worst_row > 1e-14 or abs(b_sum - 1.0) > 1e-14:
        sys.exit("ERROR: coefficients fail the consistency conditions")


if __name__ == "__main__":
    main()
