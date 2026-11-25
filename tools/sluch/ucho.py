"""Model słyszalności różnicy między dwoma nagraniami (oryginał → po filtrze).

Nie pyta „czy sygnał się zmienił”, tylko „czy człowiek to usłyszy”. Wynik w progach słyszalności (JND, just noticeable
difference): 1,0 = różnica na granicy wykrycia przez wprawnego słuchacza w bezpośrednim porównaniu A/B w ciszy.

Uproszczony model ucha (źródła w README.md):
 1. Poziom odsłuchu: sygnał o RMS 1,0 ≙ 94 dB SPL (muzyka −20 dBFS RMS ≈ 74 dB SPL – typowo w słuchawkach).
 2. Filtry słuchowe: 39 pasm o szerokości 1 ERB (Glasberg i Moore 1990), kształt roex(p) – pobudzenie błony podstawnej,
    więc słabe składowe obok silnych są maskowane.
 3. Próg słyszenia w ciszy (Terhardt 1979) jako szum wewnętrzny: zmiany poniżej progu nie liczą się.
 4. Cztery wymiary, każdy w JND:
    - głośność: różnica głośności programu (ITU-R BS.1770) / 0,8 dB;
    - barwa: różnica długookresowego wzorca pobudzenia po wyrównaniu głośności; 1 dB w czterech sąsiednich pasmach = 1 JND;
    - dynamika: zmienna w czasie część różnicy poziomów (kompresja, wyostrzanie, dodany szum w pauzach) w 8 grupach pasm,
      ramki 12 ms; odchylenie 1,5 dB w dwóch grupach = 1 JND;
    - przestrzeń: stosunek bok/środek z wewnętrznym progiem −17 dB (odpowiada progom korelacji międzyusznej:
      0,04 przy ρ = 1 i 0,35 przy ρ = 0 – Pollack i Trittipoe 1959, Gabriel i Colburn 1981), 3 dB = 1 JND;
      balans lewo–prawo 1 dB = 1 JND.
    - razem: pierwiastek z sumy kwadratów.
 5. Osobno przester (funkcja `przester`): ile wyjścia nie da się wyjaśnić liniowym filtrem – w dB, nie w JND.
Skala: < 1 niesłyszalne · 1–2 tylko w bezpośrednim A/B · 2–4 słabe · 4–8 wyraźne · > 8 bardzo mocne (README.md).
"""
import numpy as np
import pyloudnorm as pyln
import scipy.signal as sg

SPL_RMS1 = 94.0
NFFT = 2048
HOP = 512
J_GLOSNOSC = 0.8
J_BARWA = 1.0
N_BARWA = 4.0
J_DYN = 1.5
N_DYN = 2.0
J_SZER = 3.0
J_BALANS = 1.0
PROG_SM = 0.02  # −17 dB
WYMIARY = ("glosnosc", "barwa", "dynamika", "przestrzen")


def erb_num(f):
    return 21.4 * np.log10(4.37e-3 * f + 1)


def erb_num_inv(e):
    return (10 ** (e / 21.4) - 1) / 4.37e-3


def prog_ciszy_db(f):
    """Próg słyszenia w ciszy, dB SPL (Terhardt 1979)."""
    k = np.maximum(f, 20.0) / 1000.0
    return 3.64 * k ** -0.8 - 6.5 * np.exp(-0.6 * (k - 3.3) ** 2) + 1e-3 * k ** 4


class Ucho:
    def __init__(self, fs):
        self.fs = fs
        self.okno = sg.get_window("hann", NFFT)
        f = np.fft.rfftfreq(NFFT, 1 / fs)
        e = np.arange(1.5, min(40.6, erb_num(0.45 * fs)), 1.0)
        self.fc = erb_num_inv(e)
        erb = 24.7 * (4.37e-3 * self.fc + 1)
        p = 4 * self.fc / erb
        g = np.abs(f[None, :] - self.fc[:, None]) / self.fc[:, None]
        w = (1 + p[:, None] * g) * np.exp(-p[:, None] * g)
        w[:, 0] = 0
        # normalizacja (Parseval dla okna): ton o RMS 1 w środku pasma daje pobudzenie ≈ 1
        self.w = w * 2 / (NFFT * (self.okno ** 2).sum())
        self.prog = 10 ** ((prog_ciszy_db(self.fc) - SPL_RMS1) / 10)
        nb = len(self.fc)
        self.grupy = np.array_split(np.arange(nb), 8)
        self.grupy_prz = [np.where((self.fc >= a) & (self.fc < b))[0] for a, b in
                          ((150, 500), (500, 1500), (1500, 4000), (4000, 12000))]
        self.miernik = pyln.Meter(fs)

    def widmo(self, x):
        _, _, z = sg.stft(x, self.fs, window=self.okno, nperseg=NFFT, noverlap=NFFT - HOP, boundary=None, padded=False, axis=0)
        return z * self.okno.sum()  # (prążki, kanały, ramki), bez skalowania scipy

    def cechy(self, x):
        """x: (n, 2). Zwraca słownik cech nagrania."""
        z = self.widmo(x)
        l, r = z[:, 0, :], z[:, 1, :]
        m, s = 0.5 * (l + r), 0.5 * (l - r)
        pw = lambda q: self.w @ (np.abs(q) ** 2)  # (pasma, ramki)
        el, er, em, es = pw(l), pw(r), pw(m), pw(s)
        c = {"e": 0.5 * (el + er), "l": el, "r": er, "m": em, "s": es, "lufs": self.glosnosc(x)}
        c.update(self.rozpraszanie(c))
        return c

    def rozpraszanie(self, c):
        """Dwie miary z badań nad rozpraszaniem przez dźwięk (nie progi słyszalności):
        fluktuacja – głębokość wahań obwiedni 2–8 Hz w paśmie 300 Hz–4,5 kHz, dB (Schlittmeier i in. 2012: zakłócanie pracy
        rośnie z siłą fluktuacji, najwięcej przy ok. 4 Hz); srodek – poziom środka sceny w paśmie mowy względem całości, dB
        (przybliżenie stosunku wokalu do podkładu)."""
        g = np.where((self.fc >= 300) & (self.fc <= 4500))[0]
        env = np.sqrt(c["e"][g].sum(0) + self.prog[g].sum())
        env = env / env.mean()
        w = np.abs(np.fft.rfft((env - 1) * np.hanning(len(env)))) ** 2
        fm = np.fft.rfftfreq(len(env), HOP / self.fs)
        sel = (fm >= 2) & (fm <= 8)
        mod = np.sqrt(2 * w[sel].sum() / (np.hanning(len(env)) ** 2).sum() / len(env))
        srodek = 10 * np.log10((c["m"][g].sum() + 1e-30) / (c["e"].sum() + 1e-30))
        return {"fluktuacja": 20 * np.log10(mod + 1e-6), "srodek": srodek}

    def glosnosc(self, x):
        v = self.miernik.integrated_loudness(x)
        return v if np.isfinite(v) else -70.0

    def porownaj(self, a, b):
        """a – cechy oryginału, b – cechy po filtrze. Zwraca słownik: JND w wymiarach, razem i opis największej zmiany."""
        d_gl = b["lufs"] - a["lufs"]
        k = 10 ** (-d_gl / 10)  # wyrównanie głośności
        pr = self.prog[:, None]
        # barwa
        la = 10 * np.log10(a["e"].mean(1) + self.prog)
        lb = 10 * np.log10(b["e"].mean(1) * k + self.prog)
        db = lb - la
        barwa = np.sqrt((db ** 2).sum() / N_BARWA) / J_BARWA
        # dynamika
        dyn2 = 0.0
        for g in self.grupy:
            ta = 10 * np.log10(a["e"][g].sum(0) + self.prog[g].sum())
            tb = 10 * np.log10(b["e"][g].sum(0) * k + self.prog[g].sum())
            n = min(len(ta), len(tb))
            e = tb[:n] - ta[:n]
            dyn2 += (np.std(e) / J_DYN) ** 2
        dyn = np.sqrt(dyn2 / N_DYN)
        # przestrzeń
        prz2 = []
        for g in self.grupy_prz:
            if len(g) == 0:
                continue
            pg = self.prog[g].sum()

            def u(c):
                return 10 * np.log10((c["s"][g].sum() + 1e-30) / (c["m"][g].sum() + 1e-30) + PROG_SM)

            def bal(c):
                n = c["l"].shape[1]
                return 10 * np.log10((c["l"][g].sum() / n + pg) / (c["r"][g].sum() / n + pg))

            prz2.append(((u(b) - u(a)) / J_SZER) ** 2 + ((bal(b) - bal(a)) / J_BALANS) ** 2)
        prz = np.sqrt(np.mean(prz2))
        gl = abs(d_gl) / J_GLOSNOSC
        i = int(np.argmax(np.abs(db)))
        return {
            "glosnosc": gl, "barwa": barwa, "dynamika": dyn, "przestrzen": prz,
            "razem": float(np.sqrt(gl ** 2 + barwa ** 2 + dyn ** 2 + prz ** 2)),
            "d_fluktuacja": b["fluktuacja"] - a["fluktuacja"], "d_srodek": b["srodek"] - a["srodek"],
            "d_lufs": d_gl, "pasmo_hz": float(self.fc[i]), "pasmo_db": float(db[i]),
            "profil": db,
        }

    def roznica(self, x, y):
        n = min(len(x), len(y))
        return self.porownaj(self.cechy(x[:n]), self.cechy(y[:n]))


def przester(x, y, fs, nf=8192, hop=2048, b=8):
    """Zniekształcenia na muzyce, dB względem sygnału (pasmo 600 Hz–16 kHz; im niżej, tym czyściej).

    W każdej ramce (186 ms) i wąskiej grupie prążków dopasowujemy wyjście jako liniową kombinację obu kanałów wejścia
    z wolno zmiennym wzmocnieniem i opóźnieniem: Y_c ≈ (a + b·k)·X_L + (c + d·k)·X_R. Zmianę głośności, barwy, fazy
    i szerokości to dopasowanie „wyjaśnia”; zostają harmoniczne i intermodulacje (obcinanie), szybkie pompowanie wzmocnienia
    oraz dźwięk dodany (szum, długie odbicia – dlatego dla filtrów przestrzennych i Szumu tła liczba nie znaczy przesteru).
    Kotwice: lekkie obcięcie szczytów −38 dB, wyraźne −21 dB, mocne −13 dB. Poniżej 600 Hz miara jest zawodna
    (długie przesunięcia fazy zwrotnic), więc bas oceniamy osobno sygnałem wielotonowym.
    """
    n = min(len(x), len(y))
    f = np.fft.rfftfreq(nf, 1 / fs)
    ng = (len(f) - 1) // b
    st = lambda v: sg.stft(v, fs, nperseg=nf, noverlap=nf - hop, boundary=None, padded=False)[2][1:1 + ng * b].reshape(ng, b, -1)
    s = [st(x[:n, c]) for c in range(2)]
    k = (np.arange(b) - (b - 1) / 2)[None, :, None]
    a = np.moveaxis(np.stack([s[0], k * s[0], s[1], k * s[1]], -1), 1, 2)
    ah = np.conj(np.swapaxes(a, -1, -2))
    g = ah @ a
    g = g + (1e-9 * np.trace(g, axis1=-2, axis2=-1).real[..., None, None] + 1e-30) * np.eye(4)
    res = np.zeros(ng)
    tot = np.zeros(ng)
    for c in range(2):
        yy = np.moveaxis(st(y[:n, c]), 1, 2)[..., None]
        e = yy - a @ np.linalg.solve(g, ah @ yy)
        res += (np.abs(e) ** 2).sum((1, 2, 3))
        tot += (np.abs(yy) ** 2).sum((1, 2, 3))
    fg = f[1:1 + ng * b].reshape(ng, b).mean(1)
    sel = (fg >= 600) & (fg < 16000)
    return float(10 * np.log10(res[sel].sum() / (tot[sel].sum() + 1e-30) + 1e-12))


def opis(v):
    return ("niesłyszalne" if v < 1 else "tylko w A/B" if v < 2 else "słabe" if v < 4 else "wyraźne" if v < 8 else "bardzo mocne")


def kalibracja(x, fs):
    """Znane z literatury progi – czy model je odtwarza. Zwraca listę (nazwa, oczekiwanie, wynik)."""
    u = Ucho(fs)
    a = u.cechy(x)
    rng = np.random.default_rng(1)
    w = []

    def test(nazwa, ocz, y):
        r = u.porownaj(a, u.cechy(y))
        w.append((nazwa, ocz, r))

    test("to samo nagranie", "0", x.copy())
    test("odwrócona polaryzacja", "0 (niesłyszalne)", -x)
    for g in (0.3, 0.8, 2.0):
        test(f"głośniej o {g} dB", f"{g / 0.8:.1f} (próg ok. 0,5–1 dB)", x * 10 ** (g / 20))
    b_, a_ = sg.iirfilter(2, 200, btype="lowpass", fs=fs, ftype="butter")
    test("obrót fazy (all-pass 200 Hz)", "≈ 0 (faza prawie niesłyszalna)", sg.lfilter(a_[::-1], a_, x, axis=0))
    for db_, f0 in ((0.5, 4000), (1.0, 4000), (3.0, 4000), (1.0, 150), (3.0, 150)):
        typ = "high" if f0 > 1000 else "low"
        sos = sg.butter(1, f0, btype=typ, fs=fs, output="sos")
        y = x + (10 ** (db_ / 20) - 1) * sg.sosfilt(sos, x, axis=0)
        test(f"półka {'góry' if typ == 'high' else 'basu'} +{db_} dB ({f0} Hz)", {0.5: "< 1", 1.0: "≈ 1–2 (próg ok. 1 dB)", 3.0: "wyraźnie > 2"}[db_], y)
    sos = sg.butter(2, [900, 1100], btype="band", fs=fs, output="sos")
    test("wąski dzwon +1 dB przy 1 kHz", "< 1", x + 0.122 * sg.sosfilt(sos, x, axis=0))
    for lvl in (-96, -70, -50):
        test(f"biały szum {lvl} dBFS", {-96: "0 (poniżej progu)", -70: "słyszalny tylko w cichych miejscach", -50: "słyszalny"}[lvl],
             x + rng.standard_normal(x.shape) * 10 ** (lvl / 20))
    sos = sg.butter(8, 16000, fs=fs, output="sos")
    test("obcięcie powyżej 16 kHz", "< 1 (dorosły słuchacz)", sg.sosfilt(sos, x, axis=0))
    m, s = x.mean(1, keepdims=True), (x[:, :1] - x[:, 1:]) / 2
    for g in (1.0, 3.0, 6.0):
        k = 10 ** (g / 20)
        test(f"bok (L−R) +{g} dB", {1.0: "< 1", 3.0: "≈ 1", 6.0: "≈ 2"}[g], np.hstack([m + k * s, m - k * s]))
    t = np.arange(len(x)) / fs
    test("falowanie głośności ±1 dB, 0,3 Hz", "≈ 1 (na granicy)", x * 10 ** (np.sin(2 * np.pi * 0.3 * t)[:, None] / 20))
    test("falowanie głośności ±3 dB, 4 Hz", "wyraźnie > 2", x * 10 ** (3 * np.sin(2 * np.pi * 4 * t)[:, None] / 20))
    return w


if __name__ == "__main__":
    import sys
    import soundfile as sf
    sys.stdout.reconfigure(encoding="utf-8")
    x, fs = sf.read(sys.argv[1], always_2d=True)
    print(f"{'manipulacja':38} {'oczekiwane':38} {'głoś':>5} {'barwa':>5} {'dyn':>5} {'przes':>5} {'RAZEM':>6}")
    for nazwa, ocz, r in kalibracja(x, fs):
        print(f"{nazwa:38} {ocz:38} {r['glosnosc']:5.2f} {r['barwa']:5.2f} {r['dynamika']:5.2f} {r['przestrzen']:5.2f} {r['razem']:6.2f}  {opis(r['razem'])}")
