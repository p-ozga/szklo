/* Presety wizualizacji Szkła – własne, napisane dla tego programu (licencja MIT jak reszta Szkła).
   Format: obiekty presetów silnika Butterchurn z równaniami jako funkcjami (bez eval).
   Zmienne jak w MilkDrop: time, bass/mid/treb (ok. 1 = średnio), *_att (wygładzone), x, y, rad, ang (na punkt siatki). */
(function () {
  'use strict';
  const PUSTY = { baseVals: { enabled: 0 } };
  const nic = a => a;
  const sin = Math.sin, cos = Math.cos, PI = Math.PI;

  function preset(nazwa, baseVals, cz) {
    const uzup = (lista, wzor) => { const w = (lista || []).slice(0, 4); while (w.length < 4) w.push(PUSTY); return w.map(e => e.baseVals.enabled ? Object.assign({ init_eqs: nic, frame_eqs: nic }, wzor, e) : e); };
    return {
      nazwa,
      preset: {
        // wektory ruchu (siatka kresek) są w silniku domyślnie włączone – tu domyślnie wyłączone
        baseVals: Object.assign({ mv_a: 0 }, baseVals),
        init_eqs: cz.init || nic,
        frame_eqs: cz.frame || nic,
        pixel_eqs: cz.pixel || '',
        shapes: uzup(cz.shapes, {}),
        waves: uzup(cz.waves, { point_eqs: '' }),
        warp: cz.warp || '',
        comp: cz.comp || '',
      },
    };
  }
  // barwa krążąca w czasie: trzy sinusy przesunięte o 120°
  const barwa = (a, t, jasnosc = 0.5) => { a.wave_r = jasnosc + 0.5 * sin(t); a.wave_g = jasnosc + 0.5 * sin(t + 2.094); a.wave_b = jasnosc + 0.5 * sin(t + 4.189); };

  window.SzkloPresety = [
    preset('Szkło – Tunel', { decay: 0.975, wave_mode: 2, wave_a: 1.2, wave_scale: 1.1, wave_smoothing: 0.6, additivewave: 1, warp: 0.25, zoom: 1.03, darken_center: 1, gammaadj: 1.9, wave_thick: 1 }, {
      frame: a => { barwa(a, a.time * 0.6); a.q1 = 0.04 + 0.05 * (a.bass_att - 1); a.rot = 0.015 * sin(a.time * 0.21); a.cx = 0.5 + 0.08 * sin(a.time * 0.33); a.cy = 0.5 + 0.08 * cos(a.time * 0.27); return a; },
      pixel: a => { a.zoom = 1.0 + a.q1 * (0.6 + 1.6 * a.rad); return a; },
    }),
    preset('Szkło – Wir', { decay: 0.965, wave_mode: 0, wave_a: 1.5, wave_scale: 0.9, wave_smoothing: 0.5, additivewave: 1, warp: 0.1, zoom: 0.985, gammaadj: 2.0, wave_thick: 1, wave_mystery: 0.3 }, {
      frame: a => { barwa(a, a.time * 0.45 + 1.0); a.q1 = 0.05 * sin(a.time * 0.17); a.q2 = 0.02 + 0.03 * (a.mid_att - 0.8); a.zoom = 0.985 + 0.02 * (a.bass - 1); return a; },
      pixel: a => { a.rot = a.q1 + a.q2 * (1.2 - a.rad) * 2.0; return a; },
    }),
    preset('Szkło – Zorza', { decay: 0.985, wave_mode: 6, wave_a: 1.4, wave_scale: 1.6, wave_smoothing: 0.8, additivewave: 1, warp: 0.02, zoom: 1.0, gammaadj: 2.1, wave_thick: 1, wave_y: 0.5, wave_mystery: 0 }, {
      frame: a => { a.wave_r = 0.15 + 0.25 * sin(a.time * 0.31); a.wave_g = 0.75 + 0.25 * sin(a.time * 0.23); a.wave_b = 0.65 + 0.3 * cos(a.time * 0.19); a.wave_y = 0.5 + 0.18 * sin(a.time * 0.4); a.q1 = a.time * 0.7; a.q2 = 0.006 + 0.006 * a.treb_att; return a; },
      pixel: a => { a.dx = a.q2 * sin(a.y * 9 + a.q1); a.dy = -0.004 - 0.003 * sin(a.x * 6 - a.q1 * 0.7); return a; },
    }),
    preset('Szkło – Kalejdoskop', { decay: 0.97, wave_mode: 3, wave_a: 1.3, wave_scale: 1.2, wave_smoothing: 0.4, additivewave: 1, warp: 0.6, warpscale: 1.6, warpanimspeed: 0.6, zoom: 1.02, echo_zoom: 1.0, echo_alpha: 0.5, echo_orient: 3, gammaadj: 1.8, wave_thick: 1 }, {
      frame: a => { barwa(a, a.time * 0.8 + 3.0, 0.55); a.rot = 0.03 * sin(a.time * 0.13) + 0.01 * (a.bass - 1); a.zoom = 1.02 + 0.015 * sin(a.time * 0.37); a.echo_zoom = 1.0 + 0.1 * sin(a.time * 0.09); return a; },
    }),
    preset('Szkło – Puls', { decay: 0.94, wave_mode: 7, wave_a: 0.9, wave_scale: 0.8, wave_smoothing: 0.7, additivewave: 1, warp: 0.05, zoom: 0.96, gammaadj: 2.0, wave_r: 0.1, wave_g: 0.8, wave_b: 0.9, wave_mystery: -0.2 }, {
      frame: a => { a.rot = 0.01 * sin(a.time * 0.5); a.zoom = 0.96 - 0.02 * (a.bass_att - 1); a.wave_mystery = 0.4 * sin(a.time * 0.2); return a; },
      shapes: [
        { baseVals: { enabled: 1, sides: 48, additive: 1, x: 0.5, y: 0.5, rad: 0.2, r: 0.1, g: 0.75, b: 0.85, a: 0.9, r2: 0, g2: 0.1, b2: 0.2, a2: 0, border_a: 0 },
          frame_eqs: a => { a.rad = 0.08 + 0.14 * Math.max(0, a.bass - 0.6); a.r = 0.3 + 0.3 * sin(a.time * 0.7); a.g = 0.6 + 0.3 * sin(a.time * 0.5 + 2); a.b = 0.8; return a; } },
        { baseVals: { enabled: 1, sides: 6, additive: 1, thickoutline: 1, x: 0.5, y: 0.5, rad: 0.4, r: 0, g: 0, b: 0, a: 0, a2: 0, border_r: 0.2, border_g: 0.9, border_b: 1, border_a: 0.7 },
          frame_eqs: a => { a.rad = 0.3 + 0.12 * (a.mid_att - 0.8); a.ang = a.time * 0.3; a.border_a = 0.35 + 0.4 * Math.min(1, a.treb); return a; } },
      ],
    }),
    preset('Szkło – Kwiat', { decay: 0.972, wave_mode: 0, wave_a: 1.2, wave_scale: 0.6, wave_smoothing: 0.6, additivewave: 1, warp: 0.0, zoom: 1.0, gammaadj: 2.0, wave_thick: 1, wave_mystery: -0.5 }, {
      frame: a => { barwa(a, a.time * 0.5 + 5.0, 0.55); a.q1 = a.time * 0.6; a.q2 = 0.03 + 0.03 * (a.bass_att - 0.9); a.q3 = 5 + Math.floor(3.5 + 3.4 * sin(a.time * 0.05)); a.rot = 0.012 * sin(a.time * 0.11); return a; },
      pixel: a => { a.zoom = 1.0 + a.q2 * sin(a.ang * a.q3 + a.q1) + 0.012; return a; },
    }),
    preset('Szkło – Widmo', { decay: 0.955, wave_a: 0.0, warp: 0.08, zoom: 1.015, gammaadj: 2.0, darken_center: 1 }, {
      frame: a => { a.rot = 0.008 + 0.01 * sin(a.time * 0.15); a.zoom = 1.015 + 0.01 * (a.bass_att - 1); a.q1 = a.time; return a; },
      waves: [
        { baseVals: { enabled: 1, samples: 256, spectrum: 1, additive: 1, thick: 1, scaling: 1.4, smoothing: 0.6, r: 0.2, g: 0.9, b: 1, a: 1 },
          point_eqs: a => { const k = a.sample * 2 * PI + a.q1 * 0.2; const r = 0.16 + 0.35 * Math.min(1, a.value1 * 1.5); a.x = 0.5 + r * cos(k) * 0.75; a.y = 0.5 + r * sin(k); a.r = 0.3 + 0.7 * a.sample; a.g = 0.9 - 0.5 * a.sample; a.b = 1.0; return a; } },
        { baseVals: { enabled: 1, samples: 256, spectrum: 1, additive: 1, thick: 1, scaling: 1.4, smoothing: 0.6, r: 1, g: 0.5, b: 0.9, a: 0.8 },
          point_eqs: a => { const k = -a.sample * 2 * PI + a.q1 * 0.2 + PI; const r = 0.16 + 0.35 * Math.min(1, a.value2 * 1.5); a.x = 0.5 + r * cos(k) * 0.75; a.y = 0.5 + r * sin(k); a.r = 1.0; a.g = 0.3 + 0.5 * a.sample; a.b = 0.9 - 0.4 * a.sample; return a; } },
      ],
    }),
    preset('Szkło – Mgławica', { decay: 0.988, wave_mode: 5, wave_a: 0.7, wave_scale: 1.0, wave_smoothing: 0.3, additivewave: 1, wave_dots: 1, warp: 1.4, warpscale: 2.4, warpanimspeed: 0.35, zoom: 1.004, gammaadj: 2.2, wave_thick: 1 }, {
      frame: a => { barwa(a, a.time * 0.25 + 2.0, 0.6); a.rot = 0.004 * sin(a.time * 0.07); a.warp = 1.0 + 0.9 * (a.mid_att - 0.7); a.dx = 0.0015 * sin(a.time * 0.13); a.dy = 0.0015 * cos(a.time * 0.11); return a; },
    }),
  ];
})();
