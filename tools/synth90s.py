"""Synthetiseert de 90s rave-samples voor CYBERSEQ//2000.

Zelf gemaakt en daarmee vrij van rechten (CC0). Draaien met:
    python3 -m venv .venv && .venv/bin/pip install numpy scipy && .venv/bin/python tools/synth90s.py
"""

import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, lfilter

SR = 44100
OUT = "assets/samples/"


def t(dur):
    return np.arange(int(SR * dur)) / SR


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def saw(freq, dur, phase=0.0):
    f = np.broadcast_to(np.asarray(freq, dtype=float), t(dur).shape)
    ph = np.cumsum(f) / SR + phase
    return 2 * (ph % 1.0) - 1


def square(freq, dur):
    return np.sign(saw(freq, dur))


def env(dur, attack=0.002, decay=0.3):
    x = t(dur)
    a = np.clip(x / attack, 0, 1)
    return a * np.exp(-x / decay)


def lowpass(x, cutoff, order=2):
    b, a = butter(order, cutoff / (SR / 2))
    return lfilter(b, a, x)


def svf(x, cutoff, res):
    """Resonant state-variable lowpass met een cutoff per sample, voor de 303."""
    low = band = 0.0
    out = np.empty_like(x)
    q = 1.0 - res
    for i, s in enumerate(x):
        f = 2 * np.sin(np.pi * min(cutoff[i], SR / 6) / SR)
        low += f * band
        high = s - low - q * band
        band += f * high
        out[i] = low
    return out


def save(name, x):
    x = x - np.mean(x)
    x = x / (np.max(np.abs(x)) + 1e-9) * 0.9
    fade = min(len(x), 400)
    x[-fade:] *= np.linspace(1, 0, fade)
    wavfile.write(OUT + name + ".wav", SR, (x * 32767).astype(np.int16))


def supersaw(freq, dur, voices=7, detune=0.25):
    rng = np.random.default_rng(90)
    out = np.zeros(len(t(dur)))
    for v in range(voices):
        cents = (v - (voices - 1) / 2) / ((voices - 1) / 2) * detune * 100
        out += saw(freq * 2 ** (cents / 1200), dur, rng.random())
    return out / voices


# Belgische rave stab: mineur-akkoord van ontstemde saws, filter dat dichtklapt.
def rave_stab():
    dur = 0.55
    x = sum(supersaw(hz(n), dur, 3, 0.12) for n in (60, 63, 67, 70, 72))
    x += 0.5 * square(hz(48), dur)
    x = lowpass(x * env(dur, 0.001, 0.25), 4200)
    return x * env(dur, 0.001, 0.35)


# Hoover: dikke supersaw die van een octaaf lager omhoog glijdt, met vibrato.
def hoover():
    dur = 1.1
    x = t(dur)
    glide = hz(50) * 2 ** (-(12 / 12) * np.exp(-x / 0.06))
    vib = 1 + 0.012 * np.sin(2 * np.pi * 5.5 * x) * np.clip(x / 0.4, 0, 1)
    f = glide * vib
    s = supersaw(f, dur, 9, 0.45) + 0.6 * supersaw(f * 0.5, dur, 5, 0.3)
    s = lowpass(s, 3000)
    return s * np.clip(x / 0.02, 0, 1) * np.exp(-np.clip(x - 0.7, 0, None) / 0.15)


# Acid: 303-achtig riffje van vier zestienden (C2, C3, Eb2, G2 met slide).
def acid():
    step = 60 / 125 / 4
    notes = [(36, True), (48, False), (39, False), (43, True)]
    parts = []
    for i, (n, accent) in enumerate(notes):
        d = step
        x = t(d)
        f = np.full_like(x, hz(n))
        if i == 3:
            f = hz(39) + (hz(43) - hz(39)) * np.clip(x / 0.04, 0, 1)
        osc = saw(f, d)
        cut = (400 + (2600 if accent else 1400) * np.exp(-x / 0.05))
        y = svf(osc, cut, 0.86 if accent else 0.8)
        gate = np.where(x < d * 0.8, 1.0, np.linspace(1, 0, len(x)))
        parts.append(np.tanh(2.5 * y) * gate * (1.0 if accent else 0.75))
    return np.concatenate(parts)


# Reese bass: twee ontstemde saws laag, jungle-stijl.
def reese():
    dur = 1.0
    f = hz(33)
    x = saw(f * 1.004, dur) + saw(f * 0.996, dur, 0.3) + 0.5 * saw(f * 2.003, dur)
    x = lowpass(x, 500, 4)
    return np.tanh(1.8 * x) * np.clip(t(dur) / 0.01, 0, 1) * np.exp(-np.clip(t(dur) - 0.8, 0, None) / 0.08)


# M1 house orgel: kort, veel harmonischen, klassiek "Robin S".
def m1_organ():
    dur = 0.4
    x = t(dur)
    f = hz(48)
    s = sum(a * np.sin(2 * np.pi * f * h * x) for h, a in ((1, 1.0), (2, 0.7), (3, 0.5), (4, 0.4), (6, 0.25), (8, 0.15)))
    click = np.exp(-x / 0.003) * np.sin(2 * np.pi * 2400 * x) * 0.4
    return (s + click) * env(dur, 0.002, 0.12)


# House piano: Am7-akkoord met licht inharmonische partialen.
def rave_piano():
    dur = 0.9
    x = t(dur)
    out = np.zeros_like(x)
    for n in (57, 60, 64, 67, 72):
        f = hz(n)
        for k in range(1, 9):
            fk = f * k * np.sqrt(1 + 0.0004 * k * k)
            out += np.sin(2 * np.pi * fk * x) * np.exp(-x / (0.6 / k)) / k
    return out * np.clip(x / 0.003, 0, 1)


for name, fn in {
    "rave_stab": rave_stab,
    "hoover": hoover,
    "acid_303": acid,
    "reese": reese,
    "m1_organ": m1_organ,
    "rave_piano": rave_piano,
}.items():
    save(name, fn())
    print("ok", name)
