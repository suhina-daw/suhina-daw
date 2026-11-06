/*
 *      Fast Fourier Transform for the psychoacoustic model
 *
 *      This file replaces LAME 3.100's libmp3lame/fft.c, which carries no
 *      licence notice of its own and is therefore not redistributed here.
 *      It is written from the interface alone (fft.h, the fft_fht field in
 *      util.h and the call sites in psymodel.c) and a textbook description
 *      of the fast Hartley transform, so that the psychoacoustic model
 *      observes the same windowing, transform and output layout as before.
 *
 *      Contract, as consumed by psymodel.c:
 *
 *        init_fft(gfc) fills gfc->cd_psy->window[BLKSIZE] with a Blackman
 *        window and gfc->cd_psy->window_s[BLKSIZE_s / 2] with the first
 *        half of a Hann window (the second half is applied mirrored).
 *
 *        fft_long(gfc, out, chn, data) windows data[chn][0 .. BLKSIZE-1]
 *        and stores the unnormalised Hartley transform of the windowed
 *        signal in out, in natural order: out[u] holds H[u], where
 *        H[u] = sum_i w[i] x[i] (cos(2*pi*u*i/N) + sin(2*pi*u*i/N)).
 *        psymodel.c reads out[e] and out[BLKSIZE-e] as the real and
 *        imaginary parts of DFT bin e (the Hartley relation), which gives
 *        the bin energies; bin frequencies are e * samplerate / BLKSIZE.
 *
 *        fft_short(gfc, out, chn, data) does the same for three short
 *        blocks of BLKSIZE_s samples read at offsets 192, 384 and 576
 *        (BLKSIZE_s * 3/4) of data[chn], windowed by the mirrored Hann.
 *
 *      No scaling is applied and no sizes other than BLKSIZE and BLKSIZE_s
 *      are supported; both are fixed by encoder.h.
 *
 *      Copyright (C) 2026 The Suhina contributors
 *      SPDX-License-Identifier: LGPL-2.1-or-later
 */

#ifdef HAVE_CONFIG_H
# include <config.h>
#endif

#include <math.h>

#include "lame.h"
#include "machine.h"
#include "encoder.h"
#include "util.h"
#include "fft.h"


#ifndef TWO_PI
#define TWO_PI 6.283185307179586476925286766559
#endif

/*
 * Twiddle tables, shared by every transform. The butterfly of the stage
 * that merges two half transforms of size m (m = 4, 8, ..., BLKSIZE) needs
 * cos/sin of 2*pi*u/m for u = 1 .. m/2-1. One table serves both fft_long
 * (size BLKSIZE) and fft_short (size BLKSIZE_s), because the stage of
 * length m is the same in any transform large enough to contain it.
 */
#define FFT_MAX_LOG2 10                                 /* log2(BLKSIZE)     */
#define FFT_TAB_LEN  1013                               /* sum (m/2-1) above */

static double cos_tab[FFT_TAB_LEN];
static double sin_tab[FFT_TAB_LEN];
static int    stage_start[FFT_MAX_LOG2 + 1];            /* per stage 2^s */


/*
 *  In-place fast Hartley transform of n real values, n a power of two,
 *  result in natural order: x[u] <- H[u].
 *
 *  Textbook iterative scheme (Bracewell): bit-reverse permute, then merge
 *  pairs of half transforms stage by stage. The merge butterfly for a pair
 *  of indices (u, v) with u + v = m/2 inside a block of length m,
 *  given the half transforms' values
 *      p = H_e[u], q = H_o[u], r = H_e[v], t = H_o[v]
 *  and the angles B_u = 2*pi*u/m, B_v = 2*pi*v/m, produces
 *      H'[u]     = p + cos(B_u) q + sin(B_u) t
 *      H'[v]     = r + cos(B_v) t + sin(B_v) q
 *      H'[m-u]   = r + cos(B_u) t - sin(B_u) q
 *      H'[m-v]   = p + cos(B_v) q - sin(B_v) t
 *  When u == v (the middle of an even half block) the pair collapses to
 *      H'[u]     = p + (cos(B_u) + sin(B_u)) q
 *      H'[m-u]   = p + (cos(B_u) - sin(B_u)) q
 *  All four reads happen before any write, so the merge is in-place safe.
 *  The blocks' H[0] and H[m/2] entries are the even/odd sums and
 *  differences and are handled separately.
 */
static void
fht(double *x, int n)
{
    int     i, j, m, s, u, bit;

    j = 0;
    for (i = 1; i < n; i++) {
        bit = n >> 1;
        for (; (j & bit) != 0; bit >>= 1) {
            j ^= bit;
        }
        j ^= bit;
        if (i < j) {
            double const t = x[i];
            x[i] = x[j];
            x[j] = t;
        }
    }

    for (m = 2, s = 1; m <= n; m <<= 1, s++) {
        int const half = m >> 1;
        int const off = stage_start[s];
        for (i = 0; i < n; i += m) {
            double const a0 = x[i];
            double const b0 = x[i + half];
            x[i] = a0 + b0;
            x[i + half] = a0 - b0;
            for (u = 1; 2 * u < half; u++) {
                int const v = half - u;
                double const pu = cos_tab[off + u - 1];
                double const su = sin_tab[off + u - 1];
                double const pv = cos_tab[off + v - 1];
                double const sv = sin_tab[off + v - 1];
                double const p = x[i + u];
                double const q = x[i + half + u];
                double const r = x[i + v];
                double const t = x[i + half + v];
                x[i + u] = p + pu * q + su * t;
                x[i + half + v] = r + pu * t - su * q;
                x[i + v] = r + pv * t + sv * q;
                x[i + half + u] = p + pv * q - sv * t;
            }
            if ((half & 1) == 0) {
                u = half >> 1;
                if (u >= 1) {
                    double const p = x[i + u];
                    double const q = x[i + half + u];
                    double const pu = cos_tab[off + u - 1];
                    double const su = sin_tab[off + u - 1];
                    x[i + u] = p + (pu + su) * q;
                    x[i + half + u] = p + (pu - su) * q;
                }
            }
        }
    }
}


void
init_fft(lame_internal_flags * const gfc)
{
    PsyConst_t *gd = gfc->cd_psy;
    int     i, s, u;

    /*
     * Long window: Blackman over BLKSIZE samples, half a sample off the
     * grid so that the window is symmetric about its centre sample.
     */
    for (i = 0; i < BLKSIZE; i++) {
        double const phi = TWO_PI * (i + 0.5) / BLKSIZE;
        gd->window[i] = (FLOAT) (0.42 - 0.5 * cos(phi) + 0.08 * cos(2.0 * phi));
    }

    /*
     * Short window: Hann over BLKSIZE_s samples, same half-sample offset.
     * Only the first half is stored; fft_short applies the rest mirrored.
     */
    for (i = 0; i < BLKSIZE_s / 2; i++) {
        double const phi = TWO_PI * (i + 0.5) / BLKSIZE_s;
        gd->window_s[i] = (FLOAT) (0.5 - 0.5 * cos(phi));
    }

    /*
     * Twiddle tables. init_fft runs once per encoder before any transform;
     * rebuilding identical values on every call keeps concurrent encoder
     * initialisations safe: every caller finishes its own full write
     * before transforming, and later rewrites store the same numbers.
     */
    {
        int     off = 0;
        for (s = 1; s <= FFT_MAX_LOG2; s++) {
            int const half = 1 << (s - 1);
            stage_start[s] = off;
            for (u = 1; u < half; u++, off++) {
                double const phi = TWO_PI * u / (double) (1 << s);
                cos_tab[off] = cos(phi);
                sin_tab[off] = sin(phi);
            }
        }
    }
}


void
fft_long(lame_internal_flags const *const gfc, FLOAT x_real[BLKSIZE],
         int chn, const sample_t *const data[2])
{
    PsyConst_t const *gd = gfc->cd_psy;
    sample_t const *d = data[chn];
    double  y[BLKSIZE];
    int     i;

    for (i = 0; i < BLKSIZE; i++) {
        y[i] = (double) d[i] * (double) gd->window[i];
    }

    fht(y, BLKSIZE);

    for (i = 0; i < BLKSIZE; i++) {
        x_real[i] = (FLOAT) y[i];
    }
}


void
fft_short(lame_internal_flags const *const gfc, FLOAT x_real[3][BLKSIZE_s],
          int chn, const sample_t *const data[2])
{
    PsyConst_t const *gd = gfc->cd_psy;
    sample_t const *d = data[chn];
    int     b, j;

    for (b = 0; b < 3; b++) {
        /* three overlapping short blocks, each BLKSIZE_s long, stepping
           by three quarters of BLKSIZE_s */
        sample_t const *src = d + (b + 1) * (BLKSIZE_s * 3 / 4);
        double  y[BLKSIZE_s];

        for (j = 0; j < BLKSIZE_s; j++) {
            FLOAT const w = (j < BLKSIZE_s / 2)
                ? gd->window_s[j]
                : gd->window_s[BLKSIZE_s - 1 - j];
            y[j] = (double) src[j] * (double) w;
        }

        fht(y, BLKSIZE_s);

        for (j = 0; j < BLKSIZE_s; j++) {
            x_real[b][j] = (FLOAT) y[j];
        }
    }
}

/* End of fft.c */
