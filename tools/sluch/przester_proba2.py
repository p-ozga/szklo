"""Próba: zniekształcenia na MUZYCE = to, czego nie da się wyjaśnić filtrem liniowym wolno zmiennym w czasie.
W każdej ramce (186 ms) i wąskiej grupie prążków dopasowujemy Y ≈ (a + b·k)·X (wzmocnienie + opóźnienie); reszta to przester."""
import sys, subprocess, os
from pathlib import Path
import numpy as np, scipy.signal as sg, soundfile as sf
sys.stdout.reconfigure(encoding="utf-8")
ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / "target/release/szklo-cli.exe"
FR = ROOT / "tools/sluch/dane/fragmenty"
TMP = ROOT / "tools/sluch/wyniki/tmp"; TMP.mkdir(parents=True, exist_ok=True)
NF, HOP, B = 8192, 2048, 8

def przester(x, y, fs):
    """Reszta po dopasowaniu Y_c ≈ (a + b·k)·X_L + (c + d·k)·X_R w każdej ramce i grupie prążków (wzmocnienie, opóźnienie
    i mieszanie kanałów są „wyjaśnione”); to, co zostaje, to zniekształcenia nieliniowe, szybka modulacja i dodany dźwięk."""
    n = min(len(x), len(y))
    f = np.fft.rfftfreq(NF, 1 / fs); ng = (len(f) - 1) // B
    R = np.zeros(ng); T = np.zeros(ng)
    S = [sg.stft(x[:n, c], fs, nperseg=NF, noverlap=NF - HOP, boundary=None, padded=False)[2][1:1 + ng * B].reshape(ng, B, -1) for c in range(2)]
    k = (np.arange(B) - (B - 1) / 2)[None, :, None]
    A = np.stack([S[0], k * S[0], S[1], k * S[1]], -1)          # (grupa, prążek, ramka, 4)
    A = np.moveaxis(A, 1, 2)                                     # (grupa, ramka, prążek, 4)
    AH = np.conj(np.swapaxes(A, -1, -2))
    G = AH @ A
    G += (1e-9 * np.trace(G, axis1=-2, axis2=-1).real[..., None, None] + 1e-30) * np.eye(4)
    for c in range(2):
        Y = sg.stft(y[:n, c], fs, nperseg=NF, noverlap=NF - HOP, boundary=None, padded=False)[2][1:1 + ng * B].reshape(ng, B, -1)
        Y = np.moveaxis(Y, 1, 2)[..., None]
        w = np.linalg.solve(G, AH @ Y)
        E = Y - A @ w
        R += (np.abs(E) ** 2).sum((1, 2, 3)); T += (np.abs(Y) ** 2).sum((1, 2, 3))
    fg = f[1:1 + ng * B].reshape(ng, B).mean(1)
    def r(lo, hi):
        s = (fg >= lo) & (fg < hi); return 10 * np.log10(R[s].sum() / (T[s].sum() + 1e-30) + 1e-12)
    return {"cale": r(30, 16000), "bas": r(30, 150), "dol": r(150, 600), "srodek": r(600, 4000), "gora": r(4000, 16000)}

def run(f, args):
    wy = TMP / f"p_{os.getpid()}.wav"
    subprocess.run([str(CLI), str(f), str(wy), *args], check=True, capture_output=True)
    y, fs = sf.read(wy, always_2d=True); wy.unlink(); return y, fs

if __name__ == "__main__":
    frag = [FR / f"{g}_glosny.wav" for g in ("rock", "hiphop", "jazz", "classical", "electronic", "pop", "metal", "reggae")]
    def zbadaj(nazwa, args=None, fun=None):
        w = []
        for f in frag:
            x, fs = sf.read(f, always_2d=True)
            y = fun(x, fs) if fun else run(f, args)[0]
            w.append(przester(x, y, fs))
        print(f"{nazwa:30}", " ".join(f"{k}:{np.median([q[k] for q in w]):6.1f}" for k in w[0]), f" najgorszy: {max(q['cale'] for q in w):.1f}")
    if sys.argv[1] == "kontrola":
        zbadaj("to samo", fun=lambda x, fs: x.copy())
        zbadaj("ciszej 6 dB", fun=lambda x, fs: x * 0.5)
        zbadaj("obcięcie 3 dB nad RMS+9", fun=lambda x, fs: np.clip(x, -0.5, 0.5))
        zbadaj("obcięcie mocne (0,25)", fun=lambda x, fs: np.clip(x, -0.25, 0.25))
        zbadaj("obcięcie lekkie (0,8)", fun=lambda x, fs: np.clip(x, -0.8, 0.8))
        t = lambda x, fs: np.arange(len(x)) / fs
        zbadaj("wolne falowanie ±3 dB 0,5 Hz", fun=lambda x, fs: x * 10 ** (3 * np.sin(2 * np.pi * 0.5 * t(x, fs))[:, None] / 20))
        zbadaj("szybkie falowanie ±3 dB 30 Hz", fun=lambda x, fs: x * 10 ** (3 * np.sin(2 * np.pi * 30 * t(x, fs))[:, None] / 20))
        zbadaj("falowanie ±3 dB 6 Hz", fun=lambda x, fs: x * 10 ** (3 * np.sin(2 * np.pi * 6 * t(x, fs))[:, None] / 20))
        zbadaj("szum −40 dB wzgl. RMS", fun=lambda x, fs: x + np.random.default_rng(1).standard_normal(x.shape) * np.sqrt((x ** 2).mean()) * 0.01)
        for k in ("phase_rotation", "width", "space", "hood", "binaural", "less_words", "auto_eq"):
            zbadaj(f"{k} 100%", ["--only", f"{k}:1.0"])
    else:
        for k in sys.argv[2].split(","):
            for a in (0.5, 1.0):
                zbadaj(f"{k} {int(a*100)}%", ["--only", f"{k}:{a}"])
