import sys, json, os, subprocess
sys.path.insert(0, os.path.dirname(__file__))
import numpy as np, soundfile as sf
import przester_proba2 as p2
sys.stdout.reconfigure(encoding="utf-8")
p2.NF, p2.HOP, p2.B = 8192, 2048, 8
u = json.load(open(os.path.expandvars(r"%APPDATA%\Szklo\ustawienia.json"), encoding="utf-8"))
keys = [l.split()[1] for l in subprocess.run([str(p2.CLI), "--list"], capture_output=True, text=True, encoding="utf-8").stdout.splitlines() if l[:3].strip().isdigit()]
def chain(skip=(), extra=()):
    a = ["--all-off"]
    for i in range(len(u["on"])):
        if u["on"][i] and keys[i] not in skip: a += ["--module", f"{keys[i]}=on:{u['amount'][i]}"]
    for e in extra: a += ["--module", e]
    return a
frag = [p2.FR / f"{g}_{c}.wav" for g in ("rock", "hiphop", "jazz", "classical", "electronic", "pop", "metal", "reggae") for c in ("glosny", "spokojny")]
def zbadaj(nazwa, args):
    w = []; pk = []; sat = []; lu = []
    for f in frag:
        x, fs = sf.read(f, always_2d=True); y, _ = p2.run(f, args)
        w.append(p2.przester(x, y, fs)); pk.append(20 * np.log10(np.abs(y).max() + 1e-9))
        m = np.abs(y).max(); sat.append(100 * (np.abs(y) > 0.985 * m).mean()); lu.append(10 * np.log10((y ** 2).mean()) - 10 * np.log10((x ** 2).mean()))
    print(f"{nazwa:36} przester środek {np.median([q['srodek'] for q in w]):6.1f} góra {np.median([q['gora'] for q in w]):6.1f} (najgorszy {max(q['srodek'] for q in w):5.1f}) | szczyt {np.median(pk):5.1f} dBFS, max {max(pk):5.1f} | próbek przy szczycie {np.median(sat):.2f}% | głośność {np.median(lu):+.1f} dB")
print("włączone:", " ".join(f"{keys[i]}={u['amount'][i]:.2f}" for i in range(len(u["on"])) if u["on"][i]))
zbadaj("Twój zestaw", chain())
zbadaj("  bez multiband2", chain(skip=("multiband2",)))
zbadaj("  multiband1 100% zamiast mb2", chain(skip=("multiband2",), extra=("multiband1=on:1.0",)))
zbadaj("  oba multibandy 100%", chain(extra=("multiband1=on:1.0",)))
zbadaj("  + Hard Limit", chain(extra=("hard_limit=on:0.0",)))
zbadaj("  bez clipperów, + Hard Limit", chain(skip=("clipper_hd", "simple_clipper"), extra=("hard_limit=on:0.0",)))
zbadaj("  bez clipperów i limitera", chain(skip=("clipper_hd", "simple_clipper")))
zbadaj("  bez AGC", chain(skip=("agc",)))
zbadaj("  bez Punch", chain(skip=("punch",)))
zbadaj("  bez basów", chain(skip=("deep_bass", "harmonic_bass", "bass_boost", "warm_bass", "radio_bass")))
for k in ("multiband1", "multiband2", "agc", "compressor", "punch"):
    zbadaj(f"sam {k} 100%", ["--only", f"{k}:1.0"])
for p in ("codzienny", "tramwaj", "stare", "noc", "skupienie", "wyciszenie"):
    zbadaj(f"preset {p}", ["--preset", p])
