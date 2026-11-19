# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Makes the metronome's click variations from one rendered hit, as mono 32-bit float WAV files:
# python -I assets/metronome/make_variations.py assets/metronome/soft.wav assets/metronome
import wave, struct, math, sys
src, out = sys.argv[1], sys.argv[2]
w = wave.open(src); n = w.getnframes(); sr = w.getframerate()
raw = w.readframes(n); s = struct.unpack('<%df' % (n * w.getnchannels()), raw)
x = list(s[0::w.getnchannels()])

def trim(x, seconds, fade=0.01):
    n = min(len(x), int(seconds * sr)); y = x[:n]; f = int(fade * sr)
    for i in range(f):
        y[n - f + i] *= 1 - i / f
    return y
def norm(x, peak):
    p = max(abs(v) for v in x) or 1.0
    return [v * peak / p for v in x]
def lowpass(x, hz):
    a = 1 - math.exp(-2 * math.pi * hz / sr); y = []; z = 0.0
    for v in x:
        z += a * (v - z); y.append(z)
    return y
def highpass(x, hz):
    lp = lowpass(x, hz); return [a - b for a, b in zip(x, lp)]
def repitch(x, ratio):
    # Plays x at `ratio` times the speed (ratio < 1 lower and longer), linear interpolation.
    y = []; p = 0.0
    while p < len(x) - 1:
        i = int(p); f = p - i; y.append(x[i] * (1 - f) + x[i + 1] * f); p += ratio
    return y
def decay(x, seconds):
    return [v * math.exp(-i / (seconds * sr)) for i, v in enumerate(x)]
def pad(x, n):
    return x + [0.0] * max(0, n - len(x))

peak = 0.79
soft = norm(trim(x, 0.08), peak)
muted = norm(trim(lowpass(lowpass(x, 1400), 1400), 0.09), peak * 0.9)
deep = norm(trim(lowpass(repitch(x, 2 ** (-7 / 12)), 2500), 0.12), peak)
tick = norm(trim(decay(highpass(x, 2500), 0.004), 0.03, 0.005), peak * 0.8)
low = repitch(x, 0.5)
knock = norm(trim([a + 0.8 * b for a, b in zip(pad(x, len(low)), low)], 0.1), peak)

def write(name, y):
    with open(f'{out}/{name}.wav', 'wb') as f:
        data = struct.pack('<%df' % len(y), *y)
        f.write(b'RIFF' + struct.pack('<I', 36 + len(data)) + b'WAVE')
        f.write(b'fmt ' + struct.pack('<IHHIIHH', 16, 3, 1, sr, sr * 4, 4, 32))
        f.write(b'data' + struct.pack('<I', len(data)) + data)
for name, y in [('soft', soft), ('muted', muted), ('deep', deep), ('tick', tick), ('knock', knock)]:
    write(name, y); print(name, len(y), 'frames')
