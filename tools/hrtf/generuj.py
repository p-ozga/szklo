"""Tablice filtrów binauralnych dla core/src/modules/hrtf_kemar.rs.

Dane: MIT KEMAR „diffuse” (W. G. Gardner, K. D. Martin, MIT Media Lab, 1994; (c) 1994 MIT Media Laboratory, wolno używać bez
ograniczeń z podaniem autorów): https://sound.media.mit.edu/resources/KEMAR.html
Użycie: python -I tools/hrtf/generuj.py <katalog z rozpakowanym diffuse.zip>

Dla pary symetrycznych głośników ±az: w dziedzinie M/S ucho słyszy M·(Hi+Hc) oraz ±S·(Hi−Hc) (Hi – ucho bliższe,
Hc – dalsze), więc na parę wystarczą dwa filtry. Obróbka:
 - barwa: podział przez wygładzoną (1/1 oktawy) charakterystykę pary przedniej dla sumy mono – mono zostaje neutralne,
 - bas < ok. 200 Hz: czyste przejście (pomiar kończy się na ok. 200 Hz, a bas ma zostać nieruszony),
 - wspólne opóźnienie DELAY, żeby fragmenty przyczynowe i nieprzyczynowe zmieściły się w filtrze.
"""
import sys
import wave
from pathlib import Path

import numpy as np

FS0 = 44100
NF = 4096
FRONT, REAR = 45, 110
RATES = [44100, 48000, 88200, 96000]
DELAY_MS = 0.2
LEN_MS = 6.0


def wczytaj(katalog, az):
    with wave.open(str(Path(katalog) / "elev0" / f"H0e{az:03d}a.wav"), "rb") as w:
        assert w.getframerate() == FS0 and w.getnchannels() == 2 and w.getsampwidth() == 2
        d = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").reshape(-1, 2) / 32768.0
    H = np.fft.rfft(d, NF, axis=0)
    return (H[:, 1], H[:, 0]) if np.abs(H[:, 1]).sum() > np.abs(H[:, 0]).sum() else (H[:, 0], H[:, 1])


def oktawa(f, p, szer=1.0):
    out = np.empty_like(p)
    for i, x in enumerate(f):
        lo, hi = x * 2 ** (-szer / 2), x * 2 ** (szer / 2)
        s = (f >= lo) & (f <= hi)
        out[i] = p[s].mean() if s.any() else p[i]
    return out


def minfaza(mag):
    n = 2 * (len(mag) - 1)
    c = np.fft.irfft(np.log(np.maximum(mag, 1e-5)), n)
    w = np.zeros(n)
    w[0] = w[n // 2] = 1
    w[1:n // 2] = 2
    return np.exp(np.fft.rfft(c * w))


def main():
    kat = sys.argv[1]
    f = np.fft.rfftfreq(NF, 1 / FS0)
    pary = {az: wczytaj(kat, az) for az in (FRONT, REAR)}
    # minimalna faza + różnica czasu między uszami (z korelacji wzajemnej poniżej 1500 Hz); poniżej 300 Hz amplituda stała
    # (pomiar głośniczkiem nie sięga basu), oba uszy jednakowo
    nowe = {}
    for az, (hi, hc) in pary.items():
        lp = f < 1500
        kor = np.fft.irfft(np.where(lp, hc * np.conj(hi), 0), NF * 8)
        itd = np.argmax(kor[:8 * 60]) / 8 / FS0
        j = np.searchsorted(f, 300)
        mi, mc = np.abs(hi).copy(), np.abs(hc).copy()
        sr = 0.5 * (oktawa(f, mi ** 2)[j] ** 0.5 + oktawa(f, mc ** 2)[j] ** 0.5)
        x = np.clip(np.log2(np.maximum(f, 1) / 150) / np.log2(600 / 150), 0, 1)
        x = x * x * (3 - 2 * x)
        mi, mc = sr ** (1 - x) * np.maximum(mi, 1e-4) ** x, sr ** (1 - x) * np.maximum(mc, 1e-4) ** x
        print(f"{az}: ITD {itd * 1e6:.0f} us")
        nowe[az] = (minfaza(mi), minfaza(mc) * np.exp(-2j * np.pi * f * itd))
    pary = nowe
    # dane nie mają treści < 200 Hz i > 18 kHz – łagodne zszycie z poziomem obok
    ref = oktawa(f, np.abs(pary[FRONT][0] + pary[FRONT][1]) ** 2) ** 0.5
    i1 = np.searchsorted(f, 16000)
    ref[i1:] = ref[i1]
    # lekkie ściszenie > 9 kHz (różnica kanałów dostaje tam więcej niż mono)
    z = np.clip((np.log2(np.maximum(f, 1)) - np.log2(6500)) / (np.log2(11000) - np.log2(6500)), 0, 1)
    ref = ref * 10 ** (2.0 * z * z * (3 - 2 * z) / 20)
    eq = minfaza(1 / ref)
    # przyczynowe sklejenie z czystym basem: LP 1. rzędu 180 Hz i jego dopełnienie 1 − LP
    sj = 1j * f / 180.0
    lpc = 1 / (1 + sj)
    czyste = np.ones_like(f)
    wyn = {}
    for az, (a, b) in pary.items():
        for nm, H in (("M", a + b), ("S", a - b)):
            G = H * eq
            if az == FRONT and nm == "S":
                # różnica kanałów najwyżej ok. +5 dB (bez tego bardzo szerokie nagrania robią się przeciwfazowe w 400–800 Hz)
                ob = oktawa(f, np.abs(G) ** 2, 0.5) ** 0.5
                G = G * minfaza(np.minimum(1, 1.8 / np.maximum(ob, 1e-3)))
            # powyżej 18 kHz wygaszenie odchyłek
            wyn[(az, nm)] = (1 - lpc) * G + lpc * (czyste if az == FRONT else 0)
    # poziom: pasmo 300–10000 Hz średnio 0 dB dla mono z przodu
    s = (f > 300) & (f < 10000)
    g = 1 / np.sqrt((np.abs(wyn[(FRONT, "M")][s]) ** 2).mean())
    lin = []
    for (az, nm), G in wyn.items():
        G = G * ((1 - lpc) * g + lpc)
        # nadpróbkowanie przez widmo (dokładne), okno z łagodnym końcem
        for fs in RATES:
            n = int(round(LEN_MS * 1e-3 * fs))
            t = (np.arange(n) - round(DELAY_MS * 1e-3 * fs)) / fs  # opóźnienie w całych próbkach
            # wartości h0 w chwilach t (synteza z widma)
            k = np.arange(len(G))
            scale = FS0 / fs
            c = G.copy()
            c[1:] *= 2
            h = np.array([(c * np.exp(2j * np.pi * k * FS0 / NF * x)).real.sum() for x in t]) / NF * scale
            if az == FRONT:  # powyżej pasma pomiaru (22 kHz, tylko dla 48 kHz i więcej): czyste przejście
                jeden = np.full(len(G), 2.0)
                jeden[0] = 1
                bl = np.array([(jeden * np.cos(2 * np.pi * k * FS0 / NF * x)).sum() for x in t]) / NF * scale
                h -= bl
                h[round(DELAY_MS * 1e-3 * fs)] += 1
            nf = int(1.5e-3 * fs)
            h[-nf:] *= 0.5 + 0.5 * np.cos(np.pi * (np.arange(nf) + 1) / nf)
            ni = int(0.1e-3 * fs)
            h[:ni] *= 0.5 - 0.5 * np.cos(np.pi * (np.arange(ni) + 0.5) / ni)
            lin.append((az, nm, fs, h))
    out = ["// Plik wygenerowany: tools/hrtf/generuj.py – nie edytować ręcznie.",
           "//! Filtry par wirtualnych głośników w dziedzinie M/S z pomiarów HRTF MIT KEMAR",
           "//! (Bill Gardner, Keith Martin, MIT Media Lab; © 1994 MIT Media Laboratory – użycie bez ograniczeń z podaniem autorów).",
           "#![allow(clippy::excessive_precision)]", "",
           f"pub const DELAY_MS: f64 = {DELAY_MS};",
           f"/// kąt pary przedniej i tylnej w stopniach", f"pub const FRONT_DEG: u32 = {FRONT};", f"pub const REAR_DEG: u32 = {REAR};", "",
           "/// Częstotliwości próbkowania, dla których są tablice.",
           f"pub const RATES: [f64; {len(RATES)}] = [{', '.join(str(float(r)) for r in RATES)}];", "",
           "/// [częstotliwość][przód M, przód S, tył M, tył S]",
           f"pub static FIR: [[&[f32]; 4]; {len(RATES)}] = ["]
    for fs in RATES:
        out.append(f"    // {fs} Hz")
        out.append("    [")
        for az in (FRONT, REAR):
            for nm in "MS":
                h = next(x[3] for x in lin if x[:3] == (az, nm, fs))
                out.append("        &[")
                v = [f"{x:.6e}" for x in h]
                for i in range(0, len(v), 12):
                    out.append("            " + ", ".join(v[i:i + 12]) + ",")
                out.append("        ],")
        out.append("    ],")
    out.append("];")
    Path(__file__).resolve().parents[2].joinpath("core/src/modules/hrtf_kemar.rs").write_text("\n".join(out) + "\n", encoding="utf8")
    # wydruk kontrolny (48 kHz)
    for az, nm, fs, h in lin:
        if fs in (44100, 48000):
            H = np.abs(np.fft.rfft(h, 8192))
            ff = np.fft.rfftfreq(8192, 1 / fs)
            print(az, nm, fs, len(h), " ".join(f"{lo}:{10 * np.log10((H[(ff >= lo) & (ff < up)] ** 2).mean()):.1f}" for lo, up in
                  [(30, 100), (100, 200), (200, 400), (400, 800), (800, 1600), (1600, 3200), (3200, 6400), (6400, 10000), (10000, 16000), (16000, 20000)]))


main()
