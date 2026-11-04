#!/usr/bin/env python3
import argparse
import sys

import numpy as np

BAND_HZ = 44_100.0
RING_DB = -120.0
FAR_ABOVE_RIPPLE_DB = 30.0
DEEPEST_DB = -280.0
SHALLOWEST_DB = -110.0
BISECTIONS = 8
CLOSE_ENOUGH_DB = 2.0
REWEIGHTINGS = 220
SMALLEST_NODES = 600
LEAST_FAR_DELTA = 0.06
FAR_DELTA_WIDTHS = 14
NEAR_DELTA_WIDTHS = 3.5
NEAR_POINTS_PER_SIDELOBE = 10
FAR_POINTS_PER_SIDELOBE = 4
QUIETEST_WEIGHT = 1e-40
SINGULAR_BELOW = 1e-15


def even_legendre(along, terms):
    return np.polynomial.legendre.legvander(along, 2 * (terms - 1))[:, ::2]


def ripple_rows(reach, width, terms):
    farthest = max(FAR_DELTA_WIDTHS * width, LEAST_FAR_DELTA)
    nodes = int(max(SMALLEST_NODES, 2 * np.pi * farthest * reach / 1.2))
    points, weights = np.polynomial.legendre.leggauss(nodes)
    along = (points + 1) / 2
    weights = weights / 2
    legendre = even_legendre(along, terms)
    deltas = np.concatenate(
        [
            np.arange(width / 2, NEAR_DELTA_WIDTHS * width, 1 / (NEAR_POINTS_PER_SIDELOBE * reach)),
            np.arange(NEAR_DELTA_WIDTHS * width, farthest, 1 / (FAR_POINTS_PER_SIDELOBE * reach)),
        ]
    )
    integrand = np.sin(np.outer(2 * np.pi * deltas * reach, along)) / (np.pi * along)
    middle = even_legendre(np.array([0.0]), terms)[0]
    return integrand @ (weights[:, None] * legendre) - 0.5 * middle[None, :], middle


def design(reach, width, ring_taps, ripple_db, terms, ripple, middle):
    taps = np.arange(ring_taps, reach + 0.5, 1.0)
    falls = (taps - ring_taps) / (reach - ring_taps)
    far_db = ripple_db + FAR_ABOVE_RIPPLE_DB
    allowed = 10 ** ((RING_DB + (far_db - RING_DB) * falls) / 20) * (np.pi * taps)
    rows = np.vstack(
        [even_legendre(taps / reach, terms) / allowed[:, None], ripple / 10 ** (ripple_db / 20)]
    )
    basis, _ = np.linalg.qr(np.column_stack([middle, np.eye(terms)[:, 1:]]))
    particular = basis[:, 0] / (middle @ basis[:, 0])
    free = basis[:, 1:]
    weights = np.ones(len(rows)) / len(rows)
    best = None
    for _ in range(REWEIGHTINGS):
        root = np.sqrt(weights)
        step, *_ = np.linalg.lstsq(
            (rows @ free) * root[:, None], -(rows @ particular) * root, rcond=SINGULAR_BELOW
        )
        window = particular + free @ step
        errors = np.abs(rows @ window)
        worst = errors.max()
        if best is None or worst < best[0]:
            best = (worst, window.copy())
        weights = weights * errors
        weights = np.maximum(weights / weights.sum(), QUIETEST_WEIGHT)
    return best


def deepest_window(width_hz, reach, ring_taps, terms):
    width = width_hz / BAND_HZ
    ripple, middle = ripple_rows(reach, width, terms)
    found = None
    deepest, shallowest = DEEPEST_DB, SHALLOWEST_DB
    for _ in range(BISECTIONS):
        if shallowest - deepest <= CLOSE_ENOUGH_DB:
            break
        trying = (deepest + shallowest) / 2
        worst, window = design(reach, width, ring_taps, trying, terms, ripple, middle)
        if worst <= 1.0:
            found = (trying, window)
            shallowest = trying
        else:
            deepest = trying
    return found


def main():
    arguments = argparse.ArgumentParser(
        description="The even Legendre window of a level's kernel: the deepest ripple a kernel of "
        "this reach and transition can meet while ringing under -120 dB from the given tap on and "
        "falling steadily after it."
    )
    arguments.add_argument("name", help="the constant's name")
    arguments.add_argument("width_hz", type=float, help="the transition's width at a 44.1 kHz band")
    arguments.add_argument("reach", type=float, help="taps of the band either side")
    arguments.add_argument("ring_taps", type=float, help="the tap the -120 dB ring ends on")
    arguments.add_argument("terms", type=int, help="even Legendre terms")
    asked = arguments.parse_args()

    found = deepest_window(asked.width_hz, asked.reach, asked.ring_taps, asked.terms)
    if found is None:
        sys.exit("no window meets that ringing at any ripple searched")
    ripple_db, window = found
    window = window / (even_legendre(np.array([0.0]), asked.terms)[0] @ window)

    width = asked.width_hz / BAND_HZ
    print(f"ripple {ripple_db:.1f} dB, kaiser_beta {np.pi * width * asked.reach:.6f}, cutoff {1 - width:.6f}")
    print(f"const {asked.name}: [f64; {asked.terms}] = [")
    for coefficient in window:
        print(f"    {float(coefficient)!r},")
    print("];")


if __name__ == "__main__":
    main()
