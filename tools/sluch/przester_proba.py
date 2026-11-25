"""Próba zniekształceń: 30 tonów (różowe widmo, okres 2^16 próbek) – wszystko poza prążkami tonów to zniekształcenia."""
import sys, subprocess, json, os
from pathlib import Path
import numpy as np, soundfile as sf
sys.stdout.reconfigure(encoding="utf-8")
ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / "target/release/szklo-cli.exe"
TMP = ROOT / "tools/sluch/wyniki/tmp"; TMP.mkdir(parents=True, exist_ok=True)
FS, N = 48000, 1 << 16

def wieloton(lufs_ok=-14.0, seed=3):
    rng = np.random.default_rng(seed)
    f = np.geomspace(40, 12000, 30)
    k = np.unique(np.round(f * N / FS).astype(int))
    k = np.array([q + (q % 2 == 0) for q in k])  # nieparzyste prążki: produkty 2. rzędu wypadają poza tony
    k = np.unique(k)
    t = np.arange(N)
    ch = []
    for c in range(2):
        x = sum(np.sin(2 * np.pi * q * t / N + rng.uniform(0, 2 * np.pi)) / np.sqrt(q) for q in k)
        ch.append(x)
    x = np.stack(ch, 1)
    x *= 10 ** ((lufs_ok + 3) / 20) / np.sqrt((x ** 2).mean())
    return np.tile(x, (10, 1)), k

def pomiar(y, k):
    Y = np.abs(np.fft.rfft(y[-N:], axis=0)) ** 2
    Y = Y.sum(1); f = np.arange(len(Y)) * FS / N
    ton = np.zeros(len(Y), bool); ton[k] = True
    def r(lo, hi):
        s = (f >= lo) & (f < hi); return 10 * np.log10(Y[s & ~ton].sum() / (Y[s & ton].sum() + 1e-30) + 1e-12)
    return {"cale": r(20, 20000), "bas<150": r(20, 150), "150-600": r(150, 600), "600-4k": r(600, 4000), ">4k": r(4000, 20000),
            "poziom": 10 * np.log10(Y[ton].sum() / N / N * 2 + 1e-30)}

def run(args, lufs):
    x, k = wieloton(lufs)
    we, wy = TMP / "wt_we.wav", TMP / "wt_wy.wav"
    sf.write(we, x, FS, subtype="FLOAT")
    subprocess.run([str(CLI), str(we), str(wy), *args], check=True, capture_output=True)
    y, _ = sf.read(wy, always_2d=True)
    return pomiar(y, k)

def zbadaj(nazwa, args, poziomy=(-20.0, -10.0)):
    for lv in poziomy:
        w = run(args, lv)
        print(f"{nazwa:38} wej {lv:5.0f} LUFS | " + " ".join(f"{q}:{v:6.1f}" for q, v in w.items()))

if len(sys.argv) > 1 and sys.argv[1] == "moduly":
    zbadaj("(nic – kontrola)", ["--all-off"])
    for km in sys.argv[2].split(","):
        for a in (0.5, 1.0):
            zbadaj(f"{km} {int(a*100)}%", ["--only", f"{km}:{a}"])
else:
    u = json.load(open(os.path.expandvars(r"%APPDATA%\Szklo\ustawienia.json"), encoding="utf-8"))
    keys = [l.split()[1] for l in subprocess.run([str(CLI), "--list"], capture_output=True, text=True, encoding="utf-8").stdout.splitlines() if l[:3].strip().isdigit()]
    print("ustawienia:", " ".join(f"{keys[i]}={u['amount'][i]:.2f}" for i in range(len(u["on"])) if u["on"][i]))
    def chain(skip=(), extra=()):
        a = ["--all-off"]
        for i in range(len(u["on"])):
            if u["on"][i] and keys[i] not in skip: a += ["--module", f"{keys[i]}=on:{u['amount'][i]}"]
        for e in extra: a += ["--module", e]
        return a
    zbadaj("Twój zestaw", chain())
    zbadaj("  bez multiband2", chain(skip=("multiband2",)))
    zbadaj("  + multiband1 100%", chain(extra=("multiband1=on:1.0",)))
    zbadaj("  bez clipperów", chain(skip=("clipper_hd", "simple_clipper")))
    zbadaj("  bez basów", chain(skip=("deep_bass", "harmonic_bass", "bass_boost", "warm_bass", "radio_bass")))
    for p in ("codzienny", "tramwaj", "stare", "noc", "skupienie", "wyciszenie"):
        zbadaj(f"preset {p}", ["--preset", p])
