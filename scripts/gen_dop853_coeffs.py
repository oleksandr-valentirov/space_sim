#!/usr/bin/env python3
"""Генерує core/dop853_coeffs.h з вендореного джерела SciPy.

Навіщо генератор замість того, щоб набрати коефіцієнти руками: їх близько
вісімдесяти, по 27 значущих цифр. Помилка в одній цифрі не обов'язково зламає
порядок методу — вона може лише погіршити константу похибки, і тоді тест на
порядок пройде, а інтегратор мовчки працюватиме гірше. Механічна транскрипція
цей клас помилок виключає.

Згенерований заголовок комітиться, тож для збірки Python не потрібен.
Перезапускати треба лише якщо змінюється джерело.

    python3 scripts/gen_dop853_coeffs.py

numpy не потрібен: нижче мінімальна заглушка, що підтримує рівно ті операції,
які використовує вендорений файл.
"""

import sys
from pathlib import Path

SRC = Path("data/vendor/dop853_coefficients_scipy.py")
DST = Path("core/dop853_coeffs.h")


class Arr:
    """Мінімальний масив: індексація, зрізи, двовимірний доступ."""

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
    """17 значущих цифр — рівно стільки, скільки треба для round-trip double."""
    return repr(float(x)) if len(repr(float(x))) <= 24 else "%.17g" % float(x)


def main():
    if not SRC.exists():
        sys.exit("немає %s" % SRC)

    # Прибираємо import numpy: заглушка вже лежить у namespace, а справжнього
    # numpy може не бути — і ставити його заради вісімдесяти констант не варто.
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

    # Структурна перевірка тут же, щоб зіпсований заголовок не потрапив у збірку.
    worst_row = 0.0
    for i in range(n):
        row_sum = sum(a[i, j] for j in range(n))
        worst_row = max(worst_row, abs(row_sum - c[i]))
    b_sum = sum(b[i] for i in range(n))

    print("записано %s" % DST)
    print("  max |sum(A[i]) - C[i]| = %.3e" % worst_row)
    print("  |sum(B) - 1|           = %.3e" % abs(b_sum - 1.0))
    # Поріг 1e-14, а не 1e-15: рядок з 12 доданків, серед яких є ~0.9 і -0.88,
    # дає похибку сумування близько 8 ULP — виміряно 1.78e-15. Чутливості це
    # не коштує нічого: одна зіпсована цифра аж до десятого знаку зсуває суму
    # на 1e-10 і буде спіймана.
    if worst_row > 1e-14 or abs(b_sum - 1.0) > 1e-14:
        sys.exit("ПОМИЛКА: коефіцієнти не проходять умови узгодженості")


if __name__ == "__main__":
    main()
