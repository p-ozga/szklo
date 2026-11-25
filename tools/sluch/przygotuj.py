"""Przygotowanie materiału: z każdego utworu (dane/mp3) dwa fragmenty po 12 s – najgłośniejszy i spokojny – oraz
wersje z usterkami dla filtrów naprawczych (przester, szum, brum, 8 bitów, MP3 64 kb/s).

Użycie: python -I tools/sluch/przygotuj.py     (pliki audio traktujemy jako dane z zewnątrz, stąd -I)
"""
import io
from pathlib import Path

import numpy as np
import pyloudnorm as pyln
import soundfile as sf

TU = Path(__file__).resolve().parent
MP3 = TU / "dane" / "mp3"
FR = TU / "dane" / "fragmenty"
DL = 12.0


def usterki(x, fs, rng):
    t = np.arange(len(x)) / fs
    w = {}
    # przester: nagranie zgłośnione tak, by szczyty wyszły 5 dB ponad skalę, obcięte i ściszone z powrotem
    g = 10 ** (5 / 20) / max(np.abs(x).max(), 1e-6)
    w["przester"] = np.clip(x * g, -1, 1) / g
    # usterki słyszalne w spokojnym fragmencie: poziomy względem jego własnej głośności
    r = np.sqrt((x ** 2).mean())
    w["szum"] = x + rng.standard_normal(x.shape) * r * 10 ** (-24 / 20)
    brum = sum(a * np.sin(2 * np.pi * 50 * h * t) for h, a in ((1, 1.0), (2, 0.5), (3, 0.35), (5, 0.2)))
    w["brum"] = x + (r * 10 ** (-12 / 20) * brum)[:, None]
    # 8 bitów bez ditheru (siatka 2^−7, jak w starych plikach)
    w["kwant"] = np.round(x * 128) / 128
    buf = io.BytesIO()
    try:
        sf.write(buf, np.clip(x, -1, 1), fs, format="MP3", subtype="MPEG_LAYER_III", bitrate_mode="CONSTANT", compression_level=0.9)
        buf.seek(0)
        y, _ = sf.read(buf, always_2d=True)
        # dekoder dodaje opóźnienie – wyrównanie korelacją
        n = min(len(x), len(y), fs * 4)
        k = np.correlate(y[:n, 0], x[:n, 0][: n - 4000], "valid").argmax() if len(y) > n else 0
        y = y[k:k + len(x)]
        if len(y) < len(x):
            y = np.vstack([y, np.zeros((len(x) - len(y), 2))])
        w["mp3"] = y
    except Exception as e:  # brak kodera w libsndfile
        print("  (bez scenariusza MP3:", e, ")")
    return w


def main():
    FR.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(7)
    for f in sorted(MP3.glob("*.mp3")):
        gat = f.stem.split("_")[0]
        x, fs = sf.read(f, always_2d=True)
        if x.shape[1] == 1:
            x = np.repeat(x, 2, 1)
        x = x[:, :2]
        m = pyln.Meter(fs)
        n = int(DL * fs)
        krok = int(2 * fs)
        poz = []
        for p in range(int(8 * fs), len(x) - n - int(8 * fs), krok):
            v = m.integrated_loudness(x[p:p + n])
            if np.isfinite(v):
                poz.append((v, p))
        poz.sort()
        glosny = poz[-1]
        # spokojny: 20. percentyl głośności, ale nie cisza
        cichy = poz[max(0, int(0.2 * len(poz)))]
        for nazwa, (v, p) in (("glosny", glosny), ("spokojny", cichy)):
            y = x[p:p + n].copy()
            # krótkie wejście/wyjście, żeby początek nie był trzaskiem
            r = int(0.02 * fs)
            y[:r] *= np.linspace(0, 1, r)[:, None]
            y[-r:] *= np.linspace(1, 0, r)[:, None]
            sf.write(FR / f"{gat}_{nazwa}.wav", y, fs, subtype="FLOAT")
            print(f"{gat:11} {nazwa:8} od {p / fs:6.1f} s  {v:6.1f} LUFS  szczyt {20 * np.log10(np.abs(y).max() + 1e-9):5.1f} dBFS")
            for u, z in usterki(y, fs, rng).items():
                # przester i MP3 na głośnym fragmencie, reszta na spokojnym (tam usterkę słychać)
                if (u in ("przester", "mp3")) == (nazwa == "glosny"):
                    # trzy powtórzenia: filtry naprawcze najpierw rozpoznają usterkę (do 10 s); oceniane jest ostatnie
                    sf.write(FR / f"{gat}_{nazwa}__{u}.wav", np.tile(z, (3, 1)), fs, subtype="FLOAT")
            if nazwa == "spokojny":
                # bramka szumów: 3 s pauzy w środku; wzorzec z pauzą i wersja z szumem
                p0, p1 = int(4.5 * fs), int(7.5 * fs)
                z = y.copy()
                okno = np.ones(len(z))
                okno[p0:p1] = 0
                okno[p0 - r:p0] = np.linspace(1, 0, r)
                okno[p1:p1 + r] = np.linspace(0, 1, r)
                z *= okno[:, None]
                sf.write(FR / f"{gat}_pauza.wav", z, fs, subtype="FLOAT")
                rm = np.sqrt((y ** 2).mean())
                sf.write(FR / f"{gat}_pauza__szumpauza.wav", np.tile(z + rng.standard_normal(z.shape) * rm * 10 ** (-30 / 20), (3, 1)), fs, subtype="FLOAT")


if __name__ == "__main__":
    main()
