"""Badanie skuteczności filtrów Szkła miarą ludzkiego słuchu (model: ucho.py).

Każdy filtr osobno, przy kilku położeniach suwaka, na 30 fragmentach (15 gatunków × głośny i spokojny). Filtry naprawcze
dodatkowo na nagraniach z usterką (przester, szum, brum, 8 bitów, MP3) – tam liczy się, o ile wynik zbliża się do czystego
oryginału. Wynik: wyniki/<etykieta>.csv i wyniki/<etykieta>.md.

Użycie: python tools/sluch/badaj.py [--moduly klucz,klucz] [--poziomy 10,25,50,75,100] [--presety] [--etykieta nazwa]
                                    [--cli ścieżka do szklo-cli.exe] [--zestaw "klucz=on:0.5 klucz2=on:1"]
"""
import argparse
import csv
import os

for _z in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"):
    os.environ.setdefault(_z, "1")  # równoległość robią procesy, nie wątki bibliotek
import subprocess
import sys
from concurrent.futures import ProcessPoolExecutor
from functools import lru_cache
from pathlib import Path

import numpy as np
import soundfile as sf

TU = Path(__file__).resolve().parent
sys.path.insert(0, str(TU))
import ucho  # noqa: E402

ROOT = TU.parent.parent
FR = TU / "dane" / "fragmenty"
WYN = TU / "wyniki"
TMP = WYN / "tmp"
USTERKI = {"declipper": "przester", "delossifier": "mp3", "noise_reduction": "szum", "noise_gate": "szumpauza",
           "dehummer": "brum", "dequantizer": "kwant"}


@lru_cache(maxsize=None)
def ucho_dla(fs):
    return ucho.Ucho(fs)


@lru_cache(maxsize=64)
def wzor(sciezka, ostatnie=0):
    x, fs = sf.read(sciezka, always_2d=True)
    c = ucho_dla(fs).cechy(x[-ostatnie:] if ostatnie else x)
    c["n"] = len(x)
    return c


def jedno(z):
    """Jeden pomiar (w osobnym procesie)."""
    i, cli, rodzaj, klucz, p, f, ust, args = z
    f = Path(f)
    y, fs = przetworz(Path(cli), f, TMP / f"{os.getpid()}_{i}.wav", args)
    u = ucho_dla(fs)
    frag = f.stem.split("__")[0]
    gat, czesc = frag.split("_")
    cz = wzor(str(FR / f"{frag}.wav"))
    n = cz["n"]
    cy = u.cechy(y[-n:])  # nagrania z usterką są powtórzone 3× – oceniamy ostatnie powtórzenie
    xs, _ = sf.read(f, always_2d=True)
    prz = ucho.przester(xs[-n:], y[-n:], fs)
    w = {"rodzaj": rodzaj, "klucz": klucz, "poziom": p, "gatunek": gat, "czesc": czesc, "przester": prz}
    if rodzaj == "naprawa":
        cx = wzor(str(f), n)
        w.update(usterka=ust, przed=u.porownaj(cz, cx)["razem"], po=u.porownaj(cz, cy)["razem"])
        r = u.porownaj(cx, cy)
    else:
        r = u.porownaj(cz, cy)
    w.update({k: float(r[k]) for k in ("razem", "glosnosc", "barwa", "dynamika", "przestrzen", "d_lufs", "pasmo_hz", "pasmo_db", "d_fluktuacja", "d_srodek")})
    return w


def katalog(cli):
    out = subprocess.run([str(cli), "--list"], capture_output=True, text=True, encoding="utf-8").stdout
    mods, pres = [], []
    for ln in out.splitlines():
        p = ln.split()
        if ln.startswith("preset:"):
            pres.append(p[1])
        elif len(p) >= 4 and p[0].isdigit():
            mods.append((p[1], p[2], " ".join(p[3:])))
    return mods, pres


def przetworz(cli, we, wy, args):
    r = subprocess.run([str(cli), str(we), str(wy), *args], capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(f"{we.name} {args}: {r.stderr.decode('utf-8', 'replace')[-300:]}")
    y, fs = sf.read(wy, always_2d=True)
    wy.unlink(missing_ok=True)
    return y, fs


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser()
    ap.add_argument("--moduly", default="")
    ap.add_argument("--poziomy", default="10,25,50,75,100")
    ap.add_argument("--presety", action="store_true")
    ap.add_argument("--etykieta", default="pomiary")
    ap.add_argument("--cli", default=str(ROOT / "target" / "release" / "szklo-cli.exe"))
    ap.add_argument("--zestaw", default="", help="własny zestaw modułów zamiast pojedynczych filtrów")
    a = ap.parse_args()
    cli = Path(a.cli)
    mods, pres = katalog(cli)
    if a.moduly:
        mods = [m for m in mods if m[0] in a.moduly.split(",")]
    poziomy = [int(p) for p in a.poziomy.split(",")]
    TMP.mkdir(parents=True, exist_ok=True)

    czyste = sorted(f for f in FR.glob("*.wav") if "__" not in f.name and "_pauza" not in f.name)

    zadania = []
    if a.zestaw:
        args = ["--all-off"] + [v for m in a.zestaw.split() for v in ("--module", m)]
        zadania += [("zestaw", a.etykieta, 100, f, None, args) for f in czyste]
    else:
        for klucz, grupa, nazwa in mods:
            for p in poziomy:
                for f in czyste:
                    zadania.append(("filtr", klucz, p, f, None, ["--only", f"{klucz}:{p / 100}"]))
                if klucz in USTERKI:
                    for f in sorted(FR.glob(f"*__{USTERKI[klucz]}.wav")):
                        zadania.append(("naprawa", klucz, p, f, USTERKI[klucz], ["--only", f"{klucz}:{p / 100}"]))
        if a.presety:
            for k in pres:
                zadania += [("preset", k, 100, f, None, ["--preset", k]) for f in czyste]

    pelne = [(i, str(cli), r, k, p, str(f), u, ar) for i, (r, k, p, f, u, ar) in enumerate(zadania)]
    with ProcessPoolExecutor(max(2, (os.cpu_count() or 4) - 2)) as ex:
        wiersze = list(ex.map(jedno, pelne, chunksize=8))

    WYN.mkdir(exist_ok=True)
    pola = ["rodzaj", "klucz", "poziom", "gatunek", "czesc", "usterka", "przed", "po", "razem", "glosnosc", "barwa", "dynamika",
            "przestrzen", "d_lufs", "pasmo_hz", "pasmo_db", "d_fluktuacja", "d_srodek", "przester"]
    with open(WYN / f"{a.etykieta}.csv", "w", newline="", encoding="utf-8") as fh:
        wr = csv.DictWriter(fh, pola)
        wr.writeheader()
        for w in wiersze:
            wr.writerow({k: (f"{v:.3f}" if isinstance(v, float) else v) for k, v in w.items()})
    raport(wiersze, mods, poziomy, a.etykieta)


def prog(poziomy, med, cel):
    """Położenie suwaka (%), przy którym mediana przekracza `cel` JND (interpolacja liniowa od zera)."""
    px, py = [0] + list(poziomy), [0.0] + list(med)
    for i in range(1, len(px)):
        if py[i] >= cel:
            return px[i - 1] + (px[i] - px[i - 1]) * (cel - py[i - 1]) / max(py[i] - py[i - 1], 1e-9)
    return None


def raport(wiersze, mods, poziomy, etykieta):
    L = [f"# Słyszalność filtrów ({etykieta})", "",
         "Jednostka: progi słyszalności (JND). < 1 niesłyszalne · 1–2 tylko w bezpośrednim porównaniu · 2–4 słabe · "
         "4–8 wyraźne · > 8 bardzo mocne. Mediana z 30 fragmentów (15 gatunków).", "",
         "Przester (dB, pasmo 600 Hz–16 kHz; im niżej, tym czyściej): lekkie obcięcie szczytów −38, wyraźne −21, mocne −13. "
         "Dla filtrów przestrzennych (Przestrzeń, Kaptur, Binauralny) i Szumu tła liczba obejmuje celowo dodany dźwięk, "
         "nie zniekształcenia.", ""]
    f = [w for w in wiersze if w["rodzaj"] == "filtr"]
    if f:
        L += ["## Każdy filtr osobno", "",
              "| Filtr | Grupa | " + " | ".join(f"{p}%" for p in poziomy) + " | słychać od | wyraźnie od | głównie | zakres gatunków przy 100% | przester 50% / 100%, dB | ocena |",
              "|---|---|" + "---:|" * len(poziomy) + "---:|---:|---|---|---:|---|"]
        for klucz, grupa, nazwa in mods:
            med = [float(np.median([w["razem"] for w in f if w["klucz"] == klucz and w["poziom"] == p] or [0])) for p in poziomy]
            ost = [w for w in f if w["klucz"] == klucz and w["poziom"] == poziomy[-1]]
            if not ost:
                continue
            wym = max(ucho.WYMIARY, key=lambda k: np.median([w[k] for w in ost]))
            gat = {}
            for w in ost:
                gat.setdefault(w["gatunek"], []).append(w["razem"])
            g = sorted((float(np.mean(v)), k) for k, v in gat.items())
            p1, p2 = prog(poziomy, med, 1.0), prog(poziomy, med, 4.0)

            def prz(poz):
                v = [w["przester"] for w in f if w["klucz"] == klucz and w["poziom"] == poz]
                if not v:
                    return "–"
                m = float(np.median(v))
                return "czysto" if m < -60 else f"{m:.0f}"

            m = med[-1]
            ocena = ("NIESŁYSZALNY" if m < 1 else "tylko w A/B" if m < 2 else "słaby" if m < 4 else "wyraźny" if m < 8 else "bardzo mocny")
            if p1 is not None and p1 > 50:
                ocena += ", martwa połowa suwaka"
            if len(med) > 2 and med[-1] > 2 and med[1] > 0.8 * med[-1]:
                ocena += ", suwak nasyca się wcześnie"
            L.append(f"| {nazwa} | {grupa} | " + " | ".join(f"{v:.1f}" for v in med) + " | "
                     + (f"{p1:.0f}%" if p1 is not None else "nigdy") + " | " + (f"{p2:.0f}%" if p2 is not None else "nigdy")
                     + f" | {wym} | {g[0][1]} {g[0][0]:.1f} … {g[-1][1]} {g[-1][0]:.1f} | {prz(50)} / {prz(100)} | {ocena} |")
        L += ["", "## Gatunki przy suwaku 100% (razem, JND)", ""]
        gatunki = sorted({w["gatunek"] for w in f})
        L += ["| Filtr | " + " | ".join(gatunki) + " |", "|---|" + "---:|" * len(gatunki)]
        for klucz, grupa, nazwa in mods:
            v = [np.mean([w["razem"] for w in f if w["klucz"] == klucz and w["poziom"] == poziomy[-1] and w["gatunek"] == g] or [0]) for g in gatunki]
            L.append(f"| {nazwa} | " + " | ".join(f"{x:.1f}" for x in v) + " |")
    n = [w for w in wiersze if w["rodzaj"] == "naprawa"]
    if n:
        L += ["", "## Filtry naprawcze na nagraniach z usterką", "",
              "„Przed” i „po”: odległość od czystego oryginału (mniej = lepiej). „Zmiana”: jak bardzo słychać samo działanie filtra.", "",
              "| Filtr | usterka | suwak | przed | po | poprawa | zmiana |", "|---|---|---:|---:|---:|---:|---:|"]
        for klucz, grupa, nazwa in mods:
            for p in poziomy:
                r = [w for w in n if w["klucz"] == klucz and w["poziom"] == p]
                if r:
                    pr, po, zm = (float(np.median([w[k] for w in r])) for k in ("przed", "po", "razem"))
                    L.append(f"| {nazwa} | {r[0]['usterka']} | {p}% | {pr:.1f} | {po:.1f} | {pr - po:+.1f} | {zm:.1f} |")
    for rodzaj, tyt in (("preset", "Presety"), ("zestaw", "Zestaw")):
        r = [w for w in wiersze if w["rodzaj"] == rodzaj]
        if r:
            L += ["", f"## {tyt}", "", "| Nazwa | razem | głośność | barwa | dynamika | przestrzeń | zmiana LUFS | fluktuacja 2–8 Hz, dB | środek w paśmie mowy, dB | przester, dB |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
            for k in dict.fromkeys(w["klucz"] for w in r):
                q = [w for w in r if w["klucz"] == k]
                L.append(f"| {k} | " + " | ".join(f"{np.median([w[c] for w in q]):.1f}" for c in ("razem", "glosnosc", "barwa", "dynamika", "przestrzen", "d_lufs", "d_fluktuacja", "d_srodek", "przester")) + " |")
    (WYN / f"{etykieta}.md").write_text("\n".join(L) + "\n", encoding="utf-8")
    print("\n".join(L))


if __name__ == "__main__":
    main()
