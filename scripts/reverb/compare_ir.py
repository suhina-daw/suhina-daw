#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Compare known, identically excited IRs without copying any capture into source control."""

import argparse
import csv
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.io import wavfile

from analyze import analyze_ir, load_wav


def structure(samples, rate, stimulus_onset):
    """Use a common stimulus clock: never shift away a reverberator's onset delay."""
    first = round(stimulus_onset * rate)
    if not 0 <= first < len(samples):
        raise ValueError("stimulus onset outside recording")
    response = samples[first:]
    power = np.mean(response ** 2, axis=1)
    total = float(np.sum(power))
    if total < 1e-24:
        raise ValueError("silent response cannot establish structural similarity")
    onset = np.flatnonzero(np.max(np.abs(response), axis=1) > max(np.max(np.abs(response)) * 1e-4, 1e-12))
    density = []
    for ms in range(0, 601, 10):
        window = response[round(ms * rate / 1000):round((ms + 20) * rate / 1000)]
        if len(window) < 16:
            continue
        values = []
        for channel in window.T:
            std = float(np.std(channel))
            values.append(float(np.mean(np.abs(channel - np.mean(channel)) > std) / 0.3173105079) if std > 1e-15 else 0.)
        density.append({"start_ms": ms, "value": float(np.mean(values))})
    early = power[:round(rate * .1)]
    early_peak_fraction = float(np.max(early) / max(np.sum(early), 1e-30))
    remaining = np.cumsum(power[::-1])[::-1] / total
    cumulative = np.cumsum(power) / total
    arrival = {str(percent): float(np.searchsorted(cumulative, percent / 100) / rate * 1000)
               for percent in (10, 50, 90)}
    centroid = float(np.dot(np.arange(len(power), dtype=float) / rate * 1000, power) / total)
    energy_curve = [{"ms": ms, "db": float(10 * np.log10(max(remaining[min(round(ms * rate / 1000), len(remaining)-1)], 1e-12)))}
                    for ms in range(0, 8001, 5)]
    step = max(1, rate // 1000)
    envelope = [float(np.sqrt(np.mean(power[i:i+step]))) for i in range(0, min(len(power), rate), step)]
    return {"onset_ms": float(onset[0] / rate * 1000) if len(onset) else None,
            "energy_arrival_ms": arrival, "energy_centroid_ms": centroid,
            "energy_before_ms": {str(ms): float(np.sum(power[:round(ms * rate / 1000)]) / total) for ms in (10, 20, 50, 100, 200)},
            "early_peak_energy_fraction": early_peak_fraction,
            "early_crest_db": float(10*np.log10(max(np.max(early) / max(np.mean(early), 1e-30), 1e-30))),
            "density": density, "remaining_energy": energy_curve, "envelope_1ms_rms": envelope}


def octave_shape(analysis):
    energies = {key: sum(part["energy_sample_squared_seconds"] for part in values)
                for key, values in analysis["tail_spectrum_octaves_hz"].items()}
    reference = energies.get("1000", 0.)
    if reference <= 1e-30:
        return {}
    return {key: float(10*np.log10(max(value/reference, 1e-30))) for key, value in energies.items()}


def deltas(candidate, reference):
    """Separate descriptive errors; this is not a perceptual quality score."""
    a, b = candidate["analysis"], reference["analysis"]
    shape_a, shape_b = octave_shape(a), octave_shape(b)
    spectral = {key: shape_a[key]-shape_b[key] for key in shape_a.keys() & shape_b.keys()}
    da, db = candidate["structure"]["density"], reference["structure"]["density"]
    density_mae = float(np.mean([abs(x["value"]-y["value"]) for x,y in zip(da, db)]))
    decay_a, decay_b = a["broadband"]["t30"]["seconds"], b["broadband"]["t30"]["seconds"]
    return {"onset_error_ms": candidate["structure"]["onset_ms"]-reference["structure"]["onset_ms"],
            "t30_relative_error": decay_a/decay_b-1 if decay_a and decay_b else None,
            "octave_shape_error_db": spectral,
            "octave_shape_mae_db": float(np.mean(np.abs(list(spectral.values())))) if spectral else None,
            "density_mae_0_600ms": density_mae,
            "correlation_error": a["stereo_correlation"]-b["stereo_correlation"] if a["stereo_correlation"] is not None and b["stereo_correlation"] is not None else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--previous", type=Path)
    parser.add_argument("--stimulus-onset", type=float, default=1.)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not np.isfinite(args.stimulus_onset) or args.stimulus_onset < 0:
        parser.error("stimulus onset must be finite and nonnegative")
    if args.output.exists():
        parser.error("output already exists; use a fresh comparison directory")
    paths = {"reference": args.reference, "candidate": args.candidate}
    if args.previous:
        paths["previous"] = args.previous
    records, signals, rates = {}, {}, set()
    for label, path in paths.items():
        rate, samples = load_wav(path)
        rates.add(rate)
        signals[label] = samples
        analysis = analyze_ir(samples, rate)
        records[label] = {"source_name": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                          "sample_rate": rate, "channels": samples.shape[1], "analysis": analysis,
                          "structure": structure(samples, rate, args.stimulus_onset), "octave_shape_db": octave_shape(analysis)}
    if len(rates) != 1 or len({x.shape for x in signals.values()}) != 1:
        parser.error("comparison requires identical sample rates and recording dimensions")
    first = round(args.stimulus_onset * rate)
    energies = {label: float(np.mean(samples[first:]**2)) for label,samples in signals.items()}
    target = min(energies.values())
    args.output.mkdir(parents=True)
    for label, samples in signals.items():
        gain = float(np.sqrt(target / energies[label]))
        records[label]["listening_gain"] = gain
        if label != "reference":
            records[label]["difference_from_reference"] = deltas(records[label], records["reference"])
        wavfile.write(args.output/f"{label}.wav", rate, (samples*gain).astype(np.float32))
    report = {"schema": 1, "stimulus_onset_seconds": args.stimulus_onset,
              "clock": "original stimulus clock; no onset shifting or resampling",
              "listening_match": "one fixed attenuation per file to equalize full-tail mean-square energy; not perceptual loudness matching",
              "scope": "known identically excited IRs; separate diagnostics, no quality score", "responses": records}
    (args.output/"comparison.json").write_text(json.dumps(report, indent=2, allow_nan=False), encoding="utf-8")
    with (args.output/"comparison.csv").open("x", newline="", encoding="utf-8") as output:
        writer = csv.writer(output)
        writer.writerow(["response", "metric", "value"])
        for label, record in records.items():
            for key in ("onset_ms", "early_crest_db", "early_peak_energy_fraction"):
                writer.writerow([label, key, record["structure"][key]])
            writer.writerow([label, "t30_seconds", record["analysis"]["broadband"]["t30"]["seconds"]])
            writer.writerow([label, "stereo_correlation", record["analysis"]["stereo_correlation"]])
            writer.writerow([label, "energy_centroid_ms", record["structure"]["energy_centroid_ms"]])
            for percent,value in record["structure"]["energy_arrival_ms"].items():
                writer.writerow([label, f"energy_arrival_{percent}_ms", value])
            for key,value in record.get("difference_from_reference", {}).items():
                if not isinstance(value, dict): writer.writerow([label, key, value])
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
