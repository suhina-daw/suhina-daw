#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Analyze impulse WAVs, including periodic recorder captures; never infer RT60 from music.

Dependencies: numpy, scipy. See docs/spec/reverb-prototype.md.
"""

import argparse
import csv
import hashlib
import json
from pathlib import Path

import numpy as np
import scipy
from scipy import signal
from scipy.io import wavfile

MAX_BYTES = 128 * 1024 * 1024
BANDS = (125, 250, 500, 1000, 2000, 4000, 8000, 16000)


def load_wav(path):
    """Bound input before decoding; accept mono/stereo PCM and IEEE float WAV."""
    if path.stat().st_size > MAX_BYTES:
        raise ValueError("WAV exceeds 128 MiB analysis limit; split long captures")
    rate, samples = wavfile.read(path)
    if not 8000 <= rate <= 384000:
        raise ValueError("sample rate must be 8000..384000 Hz")
    if samples.ndim == 1:
        samples = samples[:, None]
    if samples.ndim != 2 or samples.shape[1] not in (1, 2) or len(samples) < 32:
        raise ValueError("expected at least 32 mono/stereo frames")
    if samples.dtype.kind == "i":
        samples = samples.astype(np.float64) / (2 ** (samples.dtype.itemsize * 8 - 1))
    elif samples.dtype == np.uint8:
        samples = (samples.astype(np.float64) - 128) / 128
    elif samples.dtype.kind == "f":
        samples = samples.astype(np.float64)
    else:
        raise ValueError("unsupported WAV sample type")
    if not np.isfinite(samples).all():
        raise ValueError("non-finite WAV samples")
    return int(rate), samples


def decay_fit(samples, rate, upper, lower):
    """Schroeder integration and linear fit; invalid estimates are explicit nulls."""
    power = np.square(samples)
    energy = np.cumsum(power[::-1])[::-1]
    invalid = {"seconds": None, "r_squared": None, "reason": "silent or too short"}
    if len(energy) < 32 or energy[0] <= 1e-24:
        return invalid
    peak = float(np.max(np.abs(samples)))
    tail_count = min(len(samples), max(32, round(rate * 0.1)))
    tail_rms = float(np.sqrt(np.mean(power[-tail_count:])))
    tail_relative_db = 20 * np.log10(max(tail_rms / max(peak, 1e-30), 1e-30))
    # Conservative: insufficient recording length/noisy tails must not look like short decay.
    if tail_relative_db > -60:
        return {**invalid, "reason": "tail not quiet enough: truncated response or noise", "tail_relative_db": float(tail_relative_db)}
    db = 10 * np.log10(np.maximum(energy / energy[0], 1e-300))
    chosen = np.flatnonzero((db <= upper) & (db >= lower))
    if len(chosen) < 16 or db[-1] > lower - 10:
        return {**invalid, "reason": "insufficient decay range"}
    x = chosen / rate
    y = db[chosen]
    slope, intercept = np.polyfit(x, y, 1)
    predicted = slope * x + intercept
    variance = float(np.sum((y - np.mean(y)) ** 2))
    r2 = 1 - float(np.sum((y - predicted) ** 2)) / max(variance, 1e-30)
    if slope >= 0 or r2 < 0.98:
        return {"seconds": None, "r_squared": float(r2), "reason": "poor decay-line fit"}
    return {"seconds": float(-60 / slope), "r_squared": float(r2), "reason": None,
            "fit_db": [upper, lower], "tail_relative_db": float(tail_relative_db)}


def decay(samples, rate):
    return {name: decay_fit(samples, rate, hi, lo)
            for name, hi, lo in (("edt", 0, -10), ("t20", -5, -25), ("t30", -5, -35))}


def analyze_ir(samples, rate):
    """Analyze an IR from its detected response onset, retaining stereo channels."""
    peak = float(np.max(np.abs(samples)))
    energy = float(np.sum(samples * samples))
    onset = np.flatnonzero(np.max(np.abs(samples), axis=1) > max(peak * 1e-4, 1e-12))
    start = int(onset[0]) if len(onset) else 0
    response = samples[start:]
    # Sum channel energies without cancelling an anti-phase stereo tail.
    envelope = np.sqrt(np.mean(response ** 2, axis=1))
    bands = {}
    tail_spectrum = {}
    for center in BANDS:
        low, high = center / np.sqrt(2), center * np.sqrt(2)
        if high >= rate * 0.49:
            continue
        sos = signal.butter(3, (low, high), btype="bandpass", fs=rate, output="sos")
        filtered = signal.sosfilt(sos, response, axis=0)
        bands[str(center)] = decay(np.sqrt(np.mean(filtered ** 2, axis=1)), rate)
        tail_spectrum[str(center)] = []
        for begin, end in ((0, 0.15), (0.15, 0.5), (0.5, 1), (1, 2), (2, 4), (4, 8)):
            section = filtered[round(begin * rate):round(end * rate)]
            total = float(np.sum(section ** 2) / response.shape[1] / rate)
            tail_spectrum[str(center)].append({"start_seconds": begin, "end_seconds": end,
                                              "energy_sample_squared_seconds": total})
    correlation = None
    if response.shape[1] == 2:
        left, right = response.T
        if np.std(left) > 1e-15 and np.std(right) > 1e-15:
            correlation = float(np.corrcoef(left, right)[0, 1])
    mono_energy = float(np.sum(np.mean(response, axis=1) ** 2))
    channel_mean_energy = float(np.sum(response ** 2) / response.shape[1])
    density = []
    for begin_ms in (0, 20, 50, 100, 200, 500):
        a, b = round(begin_ms * rate / 1000), round((begin_ms + 20) * rate / 1000)
        window = response[a:b, 0]
        if len(window) < 16:
            continue
        std = float(np.std(window))
        # Gaussian expected fraction outside +/-1 sigma is 0.31731.
        # This is a diagnostic window statistic, not an ISO echo-density certificate.
        value = float(np.mean(np.abs(window - np.mean(window)) > std) / 0.3173105079) if std > 1e-15 else None
        density.append({"start_ms": begin_ms, "duration_ms": 20, "normalized_density": value})
    return {"onset_frame": start, "peak": peak, "energy": energy,
            "duration_seconds": len(samples) / rate, "broadband": decay(envelope, rate),
            "octave_bands_hz": bands, "stereo_correlation": correlation,
            "tail_spectrum_octaves_hz": tail_spectrum,
            "mono_to_channel_mean_energy_ratio": mono_energy / channel_mean_energy if channel_mean_energy > 1e-24 else None,
            "echo_density_left": density}


def segments(samples, rate, period=None, first_onset=None):
    """Explicit periodic-impulse mode; tail validity is checked per repetition."""
    if period is None:
        return [(0, samples)]
    if not np.isfinite(period) or not 1 <= period <= 120:
        raise ValueError("period must be 1..120 seconds")
    length = round(period * rate)
    peak = np.max(np.abs(samples))
    active = np.flatnonzero(np.max(np.abs(samples), axis=1) > max(peak * 1e-4, 1e-12))
    start = round(first_onset * rate) if first_onset is not None else (int(active[0]) if len(active) else 0)
    if start < 0 or start >= len(samples):
        raise ValueError("first onset outside recording")
    # One second of pre-roll in the standard stimulus; keep 100 ms here for late starts.
    # Every period uses identical offsets; do not align each repeat to its strongest peak.
    start = max(0, start - round(0.1 * rate))
    result = [(i, samples[i:i + length]) for i in range(start, len(samples), length)
              if len(samples) - i >= max(32, round(rate * 0.5))]
    if not result:
        raise ValueError("no usable impulse period; capture at least one full tail")
    return result


def compare(candidate, baseline):
    """Descriptive differences; a shorter decay or different spectrum is not a score."""
    def ratio(a, b):
        return a / b if a is not None and b is not None and b > 0 else None

    return {"broadband_decay_ratios": {
        key: ratio(candidate["broadband"][key]["seconds"], baseline["broadband"][key]["seconds"])
        for key in ("edt", "t20", "t30")},
        "octave_t30_ratios": {
            band: ratio(data["t30"]["seconds"], baseline["octave_bands_hz"].get(band, {}).get("t30", {}).get("seconds"))
            for band, data in candidate["octave_bands_hz"].items()},
        "warning": "descriptive comparison; settings and stimulus identity must be matched explicitly"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--period-seconds", type=float)
    parser.add_argument("--first-onset-seconds", type=float)
    parser.add_argument("--baseline", type=Path, help="optional known impulse WAV for numeric ratios")
    parser.add_argument("--confirm-impulse-response", action="store_true",
                        help="required for user-supplied WAVs; arbitrary music is not an IR")
    args = parser.parse_args()
    if args.first_onset_seconds is not None and not np.isfinite(args.first_onset_seconds):
        parser.error("first onset must be finite")
    if args.input.is_file() and not args.confirm_impulse_response:
        parser.error("pass --confirm-impulse-response only for a known impulse recording")
    pattern = "*.wav" if args.confirm_impulse_response else "*-impulse-wet.wav"
    paths = sorted(args.input.glob(pattern)) if args.input.is_dir() else [args.input]
    if not paths:
        parser.error("no impulse WAVs found")
    if len(paths) > 256:
        parser.error("at most 256 WAVs per analysis run")
    baseline = None
    if args.baseline is not None:
        base_rate, base_samples = load_wav(args.baseline)
        baseline = analyze_ir(base_samples, base_rate)
    reports = []
    for path in paths:
        rate, samples = load_wav(path)
        responses = [{"recording_start_frame": start, **analyze_ir(part, rate)}
                     for start, part in segments(samples, rate, args.period_seconds, args.first_onset_seconds)]
        if baseline is not None:
            for response in responses:
                response["comparison"] = compare(response, baseline)
        reports.append({"path": str(path.resolve()), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "sample_rate": rate, "channels": samples.shape[1],
                        "responses": responses})
    report = {"schema": 1, "input_kind": "impulse-response", "fit_r_squared_min": 0.98,
              "tail_relative_db_max": -60, "band_filter": "causal Butterworth order 3 bandpass",
              "normalized": False, "files": reports,
              "baseline": str(args.baseline.resolve()) if args.baseline else None,
              "libraries": {"numpy": np.__version__, "scipy": scipy.__version__}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    with args.output.with_suffix(".csv").open("x", newline="", encoding="utf-8") as output:
        writer = csv.writer(output)
        writer.writerow(["file", "repetition", "band_hz", "metric", "seconds", "r_squared", "invalid_reason"])
        for file in reports:
            for repetition, response in enumerate(file["responses"], 1):
                for band, metrics in {"broadband": response["broadband"], **response["octave_bands_hz"]}.items():
                    for metric, result in metrics.items():
                        writer.writerow([file["path"], repetition, band, metric, result["seconds"], result["r_squared"], result["reason"]])
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
