#!/usr/bin/env python3
"""Rhythm-grade beat grid fitting for fixed-tempo tracks.

Fits a constant BPM + phase to a track by comb-filter grid search over a
spectral-flux onset envelope, then verifies alignment (median/std of strong
onsets vs the grid). DAW-rendered tracks are metronomic, so a constant grid
beats any dynamic beat tracker for judgment purposes (video-sync beat data
drifts; see disco_machine_gun: claimed 143.55 bpm vs true 144.000).

usage: beatgrid.py track.wav [bpm_hint] [--title T --artist A] > beats.json
"""

import argparse
import json
import sys
import wave

import numpy as np


def onset_envelope(pcm, sr, hop=256, win=1024, band=None):
    """Spectral-flux onset envelope; band=(lo_hz, hi_hz) restricts bins."""
    frames = np.lib.stride_tricks.sliding_window_view(pcm, win)[::hop] * np.hanning(win)
    mag = np.abs(np.fft.rfft(frames, axis=1))
    if band is not None:
        freqs = np.fft.rfftfreq(win, 1 / sr)
        mag = mag[:, (freqs >= band[0]) & (freqs <= band[1])]
    flux = np.maximum(np.diff(mag, axis=0), 0).sum(axis=1)
    flux /= flux.max() + 1e-9
    t = (np.arange(len(flux)) * hop + win / 2) / sr
    return flux, t


def fit_grid(flux, t, dur, bpm_lo, bpm_hi):
    def score(bpm, phase):
        grid = np.arange(phase, dur, 60.0 / bpm)
        idx = np.searchsorted(t, grid)
        idx = idx[(idx > 0) & (idx < len(flux))]
        return flux[idx].sum() / len(idx)

    best = (0.0, 0.0, 0.0)
    for bpm in np.arange(bpm_lo, bpm_hi, 0.01):
        for phase in np.arange(0, 60.0 / bpm, 0.005):
            s = score(bpm, phase)
            if s > best[0]:
                best = (s, bpm, phase)
    _, bpm, phase = best
    for bpm2 in np.arange(bpm - 0.02, bpm + 0.02, 0.001):
        for phase2 in np.arange(max(0, phase - 0.01), phase + 0.01, 0.001):
            s = score(bpm2, phase2)
            if s > best[0]:
                best = (s, bpm2, phase2)
    return best


def verify(flux, t, grid):
    thr = np.percentile(flux, 98)
    peaks = [
        t[i]
        for i in range(2, len(flux) - 2)
        if flux[i] > thr and flux[i] == flux[i - 2 : i + 3].max()
    ]
    peaks = np.array(peaks)
    lo = grid[np.clip(np.searchsorted(grid, peaks) - 1, 0, len(grid) - 1)]
    hi = grid[np.clip(np.searchsorted(grid, peaks), 0, len(grid) - 1)]
    off = np.where(np.abs(peaks - lo) < np.abs(peaks - hi), peaks - lo, peaks - hi)
    off = off[np.abs(off) < 0.1] * 1000
    return float(np.median(off)), float(off.std()), len(off)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("wav")
    ap.add_argument("bpm_hint", nargs="?", type=float)
    ap.add_argument("--title", default="?")
    ap.add_argument("--artist", default="moe transit authority")
    args = ap.parse_args()

    w = wave.open(args.wav)
    sr, n, ch = w.getframerate(), w.getnframes(), w.getnchannels()
    pcm = np.frombuffer(w.readframes(n), dtype="<i2")
    pcm = pcm.reshape(-1, ch).mean(axis=1) / 32768.0
    dur = len(pcm) / sr

    flux, t = onset_envelope(pcm, sr)
    if args.bpm_hint:
        lo, hi = args.bpm_hint - 1.0, args.bpm_hint + 1.0
    else:
        lo, hi = 60.0, 200.0  # slow; pass a hint when you know the ballpark
    score, bpm, phase = fit_grid(flux, t, dur, lo, hi)

    # re-anchor PHASE on the kick band: full-band flux is dominated by
    # hats/snares and happily locks onto the offbeat (disco_machine_gun
    # shipped half a beat late before this). tempo from full band, downbeat
    # phase from the kicks.
    kflux, kt = onset_envelope(pcm, sr, band=(30.0, 140.0))
    period = 60.0 / bpm
    best_k = (0.0, phase)
    for ph in np.arange(0, period, 0.002):
        grid = np.arange(ph, dur, period)
        idx = np.searchsorted(kt, grid)
        idx = idx[(idx > 0) & (idx < len(kflux))]
        sc = kflux[idx].mean()
        if sc > best_k[0]:
            best_k = (sc, ph)
    for ph in np.arange(max(0, best_k[1] - 0.004), best_k[1] + 0.004, 0.0005):
        grid = np.arange(ph, dur, period)
        idx = np.searchsorted(kt, grid)
        idx = idx[(idx > 0) & (idx < len(kflux))]
        sc = kflux[idx].mean()
        if sc > best_k[0]:
            best_k = (sc, ph)
    phase = best_k[1]

    grid = np.arange(phase, dur, 60.0 / bpm)
    med, std, cnt = verify(flux, t, grid)
    print(
        f"fit: {bpm:.3f} bpm, phase {phase * 1000:.1f} ms, score {score:.4f} | "
        f"verify: {cnt} onsets, median {med:+.1f} ms, std {std:.1f} ms",
        file=sys.stderr,
    )
    if std > 10.0:
        print("WARNING: std > 10ms — tempo may not be constant", file=sys.stderr)

    json.dump(
        {
            "title": args.title,
            "artist": args.artist,
            "bpm": round(bpm, 3),
            "offset_s": round(phase, 4),
            "duration_s": dur,
            "beat_times": [round(x, 4) for x in grid],
        },
        sys.stdout,
    )


if __name__ == "__main__":
    main()
