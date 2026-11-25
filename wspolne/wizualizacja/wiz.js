/* Wizualizacja Szkła: wspólny kod okna na PC (WebView2) i aplikacji Android (WebView).
   Silnik obrazu: Butterchurn (Jordan Berg, MIT) – otwarta implementacja formatu presetów MilkDrop w WebGL 2.
   Dźwięk nie idzie przez Web Audio: program podaje gotową falę (2 × 1024 próbki, 8 bitów) funkcją SzkloWiz.fala(). */
window.SzkloWiz = (function () {
  'use strict';
  const N = 1024;
  const S = { viz: null, canvas: null, idx: 0, raf: 0, ost: 0, ostFala: 0, ostZmiana: 0, auto: true, co: 30000, fps: 60, naNazwe: null,
              L: new Uint8Array(N).fill(128), R: new Uint8Array(N).fill(128), M: new Uint8Array(N).fill(128), cisza: new Uint8Array(N).fill(128) };
  const presety = () => window.SzkloPresety || [];

  function zaladuj(i, przejscie) {
    const p = presety();
    if (!S.viz || !p.length) return;
    S.idx = ((i % p.length) + p.length) % p.length;
    S.viz.loadPreset(p[S.idx].preset, przejscie);
    S.ostZmiana = performance.now();
    if (S.naNazwe) S.naNazwe(p[S.idx].nazwa, S.idx, p.length);
  }

  function klatka(t) {
    S.raf = requestAnimationFrame(klatka);
    if (!S.viz) return;
    const dt = (t - S.ost) / 1000;
    if (dt < 1 / S.fps - 0.002) return;
    S.ost = t;
    // bez świeżej fali dłużej niż 1,5 s (pauza, brak dźwięku) obraz dostaje ciszę i się uspokaja
    const zywa = t - S.ostFala < 1500;
    S.viz.render({ audioLevels: { timeByteArray: zywa ? S.M : S.cisza, timeByteArrayL: zywa ? S.L : S.cisza, timeByteArrayR: zywa ? S.R : S.cisza },
                   elapsedTime: Math.min(Math.max(dt, 0.001), 0.2) });
    if (S.auto && t - S.ostZmiana > S.co) zaladuj(S.idx + 1, 2.7);
  }

  return {
    /** Uruchamia wizualizację na płótnie. Zwraca tekst błędu albo null. */
    start(canvas, opcje) {
      opcje = opcje || {};
      const B = window.butterchurn && (window.butterchurn.default || window.butterchurn);
      if (!B) return 'brak silnika wizualizacji';
      try {
        const proba = document.createElement('canvas').getContext('webgl2');
        if (!proba) return 'ta karta graficzna albo przeglądarka systemowa nie obsługuje WebGL 2';
        S.canvas = canvas;
        S.fps = opcje.fps || 60;
        S.naNazwe = opcje.naNazwe || null;
        if (opcje.co) S.co = opcje.co;
        S.viz = B.createVisualizer(null, canvas, { width: canvas.width, height: canvas.height, pixelRatio: 1, textureRatio: 1 });
        zaladuj(opcje.preset != null ? opcje.preset : Math.floor(Math.random() * Math.max(1, presety().length)), 0);
        S.ost = performance.now();
        cancelAnimationFrame(S.raf);
        S.raf = requestAnimationFrame(klatka);
        return null;
      } catch (e) {
        S.viz = null;
        return 'nie udało się uruchomić wizualizacji: ' + (e && e.message ? e.message : e);
      }
    },
    stop() { cancelAnimationFrame(S.raf); S.raf = 0; },
    wznow() { if (S.viz && !S.raf) { S.ost = performance.now(); S.raf = requestAnimationFrame(klatka); } },
    dziala() { return !!S.viz && !!S.raf; },
    /** Fala od programu: base64 z 2048 bajtów (1024 lewy, 1024 prawy; środek 128). */
    fala(b64) {
      const s = atob(b64);
      if (s.length < 2 * N) return;
      for (let i = 0; i < N; i++) {
        const l = s.charCodeAt(i), r = s.charCodeAt(N + i);
        S.L[i] = l; S.R[i] = r; S.M[i] = (l + r) >> 1;
      }
      S.ostFala = performance.now();
    },
    /** Fala wprost z tablic (strona testowa). */
    falaTablice(l, r) { for (let i = 0; i < N; i++) { S.L[i] = l[i]; S.R[i] = r[i]; S.M[i] = (l[i] + r[i]) >> 1; } S.ostFala = performance.now(); },
    rozmiar(w, h) {
      if (!S.canvas) return;
      w = Math.max(16, Math.round(w)); h = Math.max(16, Math.round(h));
      if (S.canvas.width === w && S.canvas.height === h) return;
      S.canvas.width = w; S.canvas.height = h;
      if (S.viz) S.viz.setRendererSize(w, h);
    },
    nastepny(krok) { zaladuj(S.idx + (krok == null ? 1 : krok), 1.5); },
    auto(b) { S.auto = !!b; S.ostZmiana = performance.now(); },
    nazwa() { const p = presety(); return p.length ? p[S.idx].nazwa : ''; },
  };
})();
