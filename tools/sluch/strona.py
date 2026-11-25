"""Strona z wynikami badania słyszalności: wyniki/<etykieta>.csv → wyniki/<etykieta>.html (jeden plik, bez zależności).

Użycie: python tools/sluch/strona.py v051 [etykieta pomiaru „przed strojeniem”, np. v050_przed]
"""
import csv
import json
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

TU = Path(__file__).resolve().parent
ROOT = TU.parent.parent
WYN = TU / "wyniki"
PRZESTRZENNE = {"space", "hood", "binaural", "bed", "width", "less_words"}


def main():
    et = sys.argv[1]
    przed_et = sys.argv[2] if len(sys.argv) > 2 else None
    r = list(csv.DictReader(open(WYN / f"{et}.csv", encoding="utf-8")))
    for w in r:
        for k, v in w.items():
            if k not in ("rodzaj", "klucz", "gatunek", "czesc", "usterka") and v not in ("", None):
                w[k] = float(v)
    out = subprocess.run([str(ROOT / "target/release/szklo-cli.exe"), "--list"], capture_output=True, text=True, encoding="utf-8").stdout
    mods = [(p[1], p[2], " ".join(p[3:])) for p in (ln.split() for ln in out.splitlines()) if len(p) >= 4 and p[0].isdigit()]
    presety = dict(re.findall(r"preset: (\w+) \((.+)\)", out))
    med = lambda v: round(float(np.median(v)), 2) if len(v) else None
    f = [w for w in r if w["rodzaj"] == "filtr"]
    poziomy = sorted({int(w["poziom"]) for w in f})
    gatunki = sorted({w["gatunek"] for w in f})
    przed = {}
    if przed_et and (WYN / f"{przed_et}.md").exists():
        for ln in (WYN / f"{przed_et}.md").read_text(encoding="utf-8").splitlines():
            c = [x.strip() for x in ln.split("|")]
            if len(c) > 9 and c[2] in ("Naprawa", "Dynamika", "Bas", "Barwa", "Stereo", "Wyjście"):
                przed[c[1]] = float(c[7])
    filtry = []
    for klucz, grupa, nazwa in mods:
        q = [w for w in f if w["klucz"] == klucz]
        if not q:
            continue
        ost = [w for w in q if w["poziom"] == poziomy[-1]]
        wym = max(("glosnosc", "barwa", "dynamika", "przestrzen"), key=lambda k: np.median([w[k] for w in ost]))
        filtry.append({
            "klucz": klucz, "grupa": grupa, "nazwa": nazwa,
            "jnd": [med([w["razem"] for w in q if w["poziom"] == p]) for p in poziomy],
            "wym": {"glosnosc": "głośność", "barwa": "barwa", "dynamika": "dynamika", "przestrzen": "przestrzeń"}[wym],
            "gat": [round(float(np.mean([w["razem"] for w in ost if w["gatunek"] == g])), 1) for g in gatunki],
            "przester": [med([w["przester"] for w in q if w["poziom"] == p]) for p in (50, 100)],
            "przestrzenny": klucz in PRZESTRZENNE,
            "przed": przed.get(nazwa),
            "fluktuacja": med([w["d_fluktuacja"] for w in ost]),
        })
    naprawa = []
    for klucz, grupa, nazwa in mods:
        for p in poziomy:
            q = [w for w in r if w["rodzaj"] == "naprawa" and w["klucz"] == klucz and w["poziom"] == p]
            if q:
                naprawa.append({"nazwa": nazwa, "usterka": q[0]["usterka"], "poziom": p, "przed": med([w["przed"] for w in q]),
                                "po": med([w["po"] for w in q]),
                                "najlepiej": max(((w["przed"] - w["po"], w["gatunek"], w["przed"], w["po"]) for w in q))[1:]})
    pres = []
    for k in dict.fromkeys(w["klucz"] for w in r if w["rodzaj"] == "preset"):
        q = [w for w in r if w["rodzaj"] == "preset" and w["klucz"] == k]
        pres.append({"nazwa": presety.get(k, k), **{c: med([w[c] for w in q]) for c in
                     ("razem", "glosnosc", "barwa", "dynamika", "przestrzen", "d_lufs", "d_fluktuacja", "d_srodek", "przester")}})
    utwory = list(csv.DictReader(open(TU / "utwory.csv", encoding="utf-8")))
    wersja = re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(encoding="utf-8"), re.M).group(1)
    dane = {"wersja": wersja, "poziomy": poziomy, "gatunki": gatunki, "filtry": filtry, "naprawa": naprawa, "presety": pres, "utwory": utwory}
    html = (TU / "strona_szablon.html").read_text(encoding="utf-8").replace("/*DANE*/null", json.dumps(dane, ensure_ascii=False))
    (WYN / f"{et}.html").write_text(html, encoding="utf-8")
    print("zapisano", WYN / f"{et}.html", len(html) // 1024, "KB")


if __name__ == "__main__":
    main()
