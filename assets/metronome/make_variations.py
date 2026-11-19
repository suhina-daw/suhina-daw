# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Makes the metronome's click variations from one rendered hit, as mono 32-bit float WAV files:
# python -I assets/metronome/make_variations.py assets/metronome/soft.wav assets/metronome
import struct, math, sys


def read_float_wav(path):
    # Python's wave module gained IEEE-float support only in 3.15. These source files
    # use format 3, so read their RIFF chunks explicitly on older Python versions too.
    with open(path, 'rb') as source:
        raw = source.read(16 * 1024 * 1024 + 1)
    if len(raw) > 16 * 1024 * 1024 or len(raw) < 12 or raw[:4] != b'RIFF' or raw[8:12] != b'WAVE':
        raise ValueError('expected a RIFF WAV of at most 16 MB')
    end = 8 + struct.unpack_from('<I', raw, 4)[0]
    if end != len(raw):
        raise ValueError('incorrect RIFF length')
    fmt = data = None
    at = 12
    while at < end:
        if at + 8 > end:
            raise ValueError('truncated chunk header')
        kind, size = struct.unpack_from('<4sI', raw, at)
        at += 8
        if at + size + size % 2 > end:
            raise ValueError('truncated chunk')
        if kind == b'fmt ':
            fmt = raw[at:at + size]
        elif kind == b'data':
            data = raw[at:at + size]
        at += size + size % 2
    if fmt is None or len(fmt) < 16 or data is None:
        raise ValueError('missing format or samples')
    tag, channels, rate, byte_rate, align, bits = struct.unpack_from('<HHIIHH', fmt)
    if (tag != 3 or bits != 32 or not 1 <= channels <= 32 or not 1000 <= rate <= 384000
            or align != channels * 4 or byte_rate != rate * align or len(data) % align):
        raise ValueError('expected whole frames of 32-bit IEEE-float audio')
    samples = struct.unpack('<%df' % (len(data) // 4), data)[::channels]
    if len(samples) < 2 or not all(math.isfinite(v) for v in samples):
        raise ValueError('expected at least two finite sample frames')
    return rate, list(samples)


src, out = sys.argv[1], sys.argv[2]
sr, x = read_float_wav(src)

def trim(x, seconds, fade=0.01):
    n = min(len(x), int(seconds * sr)); y = x[:n]; f = min(n, int(fade * sr))
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
for name, y in [('muted', muted), ('deep', deep), ('tick', tick), ('knock', knock)]:
    write(name, y); print(name, len(y), 'frames')
