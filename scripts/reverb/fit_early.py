#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Fit early/late balance from native zero/unit early-path renders, not waveform samples."""

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import scipy
from scipy.optimize import least_squares

from analyze import load_wav

WINDOWS_MS = (10, 20, 50, 100, 200)


def fit_early(reference, late, unit, rate, basis_width, onset=1.):
    """Fit energy fractions and channel correlation; basis settings must otherwise match."""
    if not np.isfinite(basis_width) or not 0 < basis_width <= 1:
        raise ValueError("basis width must be finite, greater than zero and at most one")
    if not np.isfinite(onset) or onset < 0:
        raise ValueError("stimulus onset must be finite and nonnegative")
    if not 8000 <= rate <= 384000:
        raise ValueError("unsupported sample rate")
    if reference.shape != late.shape or unit.shape != late.shape or late.ndim != 2 or late.shape[1] != 2:
        raise ValueError("expected identical stereo recording dimensions")
    if not all(np.isfinite(x).all() for x in (reference, late, unit)):
        raise ValueError("non-finite input")
    start = round(onset * rate)
    if len(late)-start < round(rate*.25):
        raise ValueError("need at least 250 ms after stimulus onset")
    reference, late, early = reference[start:], late[start:], (unit-late)[start:]
    if min(float(np.sum(x*x)) for x in (reference, late, early)) < 1e-24:
        raise ValueError("silent or missing independent early/late basis")
    components = np.stack([late.mean(axis=1), early.mean(axis=1),
                           (late[:,0]-late[:,1])*.5/basis_width,
                           (early[:,0]-early[:,1])*.5/basis_width], axis=1)
    total = components.T @ components
    centered = components - components.mean(axis=0)
    covariance = centered.T @ centered
    partials = []
    for ms in WINDOWS_MS:
        section = components[:round(rate*ms/1000)]
        partials.append(section.T @ section)
    power = np.mean(reference*reference,axis=1)
    targets = np.array([np.sum(power[:round(rate*ms/1000)])/np.sum(power) for ms in WINDOWS_MS])
    if min(np.std(reference,axis=0)) < 1e-15:
        raise ValueError("reference channel has no measurable variance")
    target_correlation = float(np.corrcoef(reference.T)[0,1])

    def vectors(gain):
        return np.array([1.,gain,0.,0.]),np.array([0.,0.,1.,gain])

    def energy(matrix,gain,width):
        mid,side=vectors(gain)
        return mid@matrix@mid + width*width*(side@matrix@side)

    def correlation(gain,width):
        mid,side=vectors(gain)
        em=mid@covariance@mid
        es=width*width*(side@covariance@side)
        cross=width*(mid@covariance@side)
        return float((em-es)/np.sqrt(max((em+es)**2-4*cross*cross,1e-60)))

    def fractions(gain,width):
        return np.array([energy(matrix,gain,width)/max(energy(total,gain,width),1e-30) for matrix in partials])

    def residual(values):
        gain,width=values
        return np.r_[(fractions(gain,width)-targets)*np.array([1.,1.,2.,1.,1.]),
                     2*(correlation(gain,width)-target_correlation)]

    result=least_squares(residual,[.4,basis_width],bounds=([0.,.05],[2.,1.]),
                         ftol=1e-12,xtol=1e-12,gtol=1e-12)
    if not result.success or np.linalg.matrix_rank(result.jac) < 2:
        raise ValueError("early level and width are not independently identifiable")
    gain,width=map(float,result.x)
    return {"early_level":gain,"width":width,"basis_width":basis_width,
            "libraries":{"numpy":np.__version__,"scipy":scipy.__version__},
            "stimulus_onset_seconds":onset,"windows_ms":WINDOWS_MS,
            "reference_energy_fractions":targets.tolist(),
            "predicted_energy_fractions":fractions(gain,width).tolist(),
            "reference_correlation":target_correlation,"predicted_correlation":correlation(gain,width),
            "weighted_diagnostic_residuals":residual(result.x).tolist(),
            "method":"bounded early-level/width fit on cumulative channel energy and correlation; no waveform-sample matching or perceptual score",
            "verification_required":"render the fitted settings with the native core; confirm basis prediction, decay, density, tone and CPU separately"}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference",type=Path)
    parser.add_argument("late",type=Path,help="native early_level=0 render")
    parser.add_argument("unit",type=Path,help="native early_level=1 render; otherwise identical settings")
    parser.add_argument("--basis-width",type=float,required=True)
    parser.add_argument("--stimulus-onset",type=float,default=1.)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    if args.output.exists(): parser.error("output already exists; choose a fresh report")
    inputs=[load_wav(path) for path in (args.reference,args.late,args.unit)]
    if len({rate for rate,_ in inputs})!=1: parser.error("sample rates must match")
    report=fit_early(*(samples for _,samples in inputs),inputs[0][0],args.basis_width,args.stimulus_onset)
    report["sources"]=[{"name":path.name,"sha256":hashlib.sha256(path.read_bytes()).hexdigest()}
                       for path in (args.reference,args.late,args.unit)]
    args.output.parent.mkdir(parents=True,exist_ok=True)
    with args.output.open("x",encoding="utf-8") as output:
        json.dump(report,output,indent=2,allow_nan=False)
    print(f"Wrote {args.output}")


if __name__=="__main__": main()
