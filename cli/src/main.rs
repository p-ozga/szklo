//! szklo-cli <we.wav> <wy.wav> [--preset KLUCZ] [--all-off] [--only klucz:0.6] [--module klucz=on:0.6|off]...
//!           [--master 0.8] [--ceiling -1.0] [--no-compensate] [--low-latency] [--list]
//! Render offline: moduły ustawione od pierwszej próbki (bez przenikania), latencja toru kompensowana
//! (wyjście wyrównane z wejściem, ta sama długość), format wyjścia = format wejścia (int 16/24/32 albo float 32).
#![forbid(unsafe_code)]

use std::process::ExitCode;
use std::sync::Arc;
use szklo_core::{Ctx, Engine, ModuleId, SharedMeters, SharedParams, MODULES};

fn usage() -> ExitCode {
    eprintln!(
        "Użycie: szklo-cli <we.wav> <wy.wav> [--preset codzienny|tramwaj|stare|noc] [--all-off] \
         [--only klucz:moc] [--module klucz=on:moc|off]... [--master m] [--ceiling dB] \
         [--no-compensate] [--low-latency] [--list]"
    );
    ExitCode::from(2)
}

/// "0.6", "60", "60%" → 0.6
fn parse_amount(s: &str) -> Option<f32> {
    let pct = s.ends_with('%');
    let v: f32 = s.trim_end_matches('%').parse().ok()?;
    Some(if pct || v > 1.0 { v / 100.0 } else { v })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        for m in MODULES.iter() {
            println!("{:>2}  {:<18} {:<10} {}", m.id.index(), m.key, m.group.name_pl(), m.name_pl);
        }
        for p in szklo_core::PRESETS.iter() {
            println!("preset: {} ({})", p.key, p.name_pl);
        }
        return ExitCode::SUCCESS;
    }

    let params = Arc::new(SharedParams::default());
    let meters = Arc::new(SharedMeters::new());
    params.apply_preset(&szklo_core::PRESETS[0]); // domyślnie „Codzienny”
    let mut compensate = true;
    let mut low_latency = false;
    let mut pos: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--preset" => {
                let Some(k) = it.next() else { return usage() };
                let Some(p) = szklo_core::presets::by_key(k) else {
                    eprintln!("Nieznany preset: {k}");
                    return ExitCode::from(2);
                };
                params.apply_preset(p);
            }
            "--all-off" => {
                for m in MODULES.iter() {
                    params.set_module_on(m.id.index(), false);
                }
            }
            "--only" => {
                let Some(v) = it.next() else { return usage() };
                let (k, am) = v.split_once(':').unwrap_or((v.as_str(), "0.5"));
                let (Some(id), Some(am)) = (ModuleId::from_key(k), parse_amount(am)) else {
                    eprintln!("Zły argument --only: {v} (zob. --list)");
                    return ExitCode::from(2);
                };
                for m in MODULES.iter() {
                    params.set_module_on(m.id.index(), false);
                }
                params.set_module(id.index(), true, am);
            }
            "--module" => {
                let Some(v) = it.next() else { return usage() };
                let Some((k, rest)) = v.split_once('=') else { return usage() };
                let Some(id) = ModuleId::from_key(k) else {
                    eprintln!("Nieznany moduł: {k} (zob. --list)");
                    return ExitCode::from(2);
                };
                if rest == "off" {
                    params.set_module_on(id.index(), false);
                } else {
                    let am = rest.strip_prefix("on:").or_else(|| rest.strip_prefix("on")).unwrap_or(rest);
                    let am = if am.is_empty() {
                        params.module(id.index()).1
                    } else {
                        match parse_amount(am) {
                            Some(x) => x,
                            None => return usage(),
                        }
                    };
                    params.set_module(id.index(), true, am);
                }
            }
            "--master" => {
                let Some(v) = it.next().and_then(|v| parse_amount(v)) else { return usage() };
                params.set_master(v);
            }
            "--ceiling" => {
                let Some(v) = it.next().and_then(|v| v.parse::<f32>().ok()) else { return usage() };
                params.set_ceiling_dbtp(v);
            }
            "--no-compensate" => compensate = false,
            "--low-latency" => low_latency = true,
            s if s.starts_with("--") => {
                eprintln!("Nieznana opcja: {s}");
                return usage();
            }
            _ => pos.push(a.clone()),
        }
    }
    if pos.len() < 2 {
        return usage();
    }

    let mut reader = match hound::WavReader::open(&pos[0]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Nie mogę otworzyć {}: {e}", pos[0]);
            return ExitCode::from(1);
        }
    };
    let spec = reader.spec();
    let ch = (spec.channels as usize).max(1);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap_or(0) as f32 * scale).collect()
        }
    };
    let frames = samples.len() / ch;
    let rc = if ch > 1 { 1 } else { 0 };
    let mut l: Vec<f32> = (0..frames).map(|f| samples[f * ch]).collect();
    let mut r: Vec<f32> = (0..frames).map(|f| samples[f * ch + rc]).collect();

    let ctx = Ctx { fs: spec.sample_rate as f64, low_latency };
    let mut engine = Engine::with_ctx(ctx, params.clone(), meters.clone());
    engine.force_current_state();
    let lat = if compensate { engine.latency() } else { 0 };
    l.extend(std::iter::repeat(0.0).take(lat));
    r.extend(std::iter::repeat(0.0).take(lat));

    let t0 = std::time::Instant::now();
    for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        engine.process(cl, cr);
    }
    let dt = t0.elapsed().as_secs_f64();
    let dur = frames as f64 / ctx.fs;

    let mut writer = match hound::WavWriter::create(&pos[1], spec) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Nie mogę zapisać {}: {e}", pos[1]);
            return ExitCode::from(1);
        }
    };
    let maxv = ((1u64 << (spec.bits_per_sample.saturating_sub(1))) - 1) as f32;
    for f in 0..frames {
        for c in 0..ch {
            let v = match c {
                0 => l[f + lat],
                1 => r[f + lat],
                _ => samples[f * ch + c],
            };
            let res = match spec.sample_format {
                hound::SampleFormat::Float => writer.write_sample(v),
                hound::SampleFormat::Int => writer.write_sample((v.clamp(-1.0, 1.0) * maxv).round() as i32),
            };
            if let Err(e) = res {
                eprintln!("Błąd zapisu: {e}");
                return ExitCode::from(1);
            }
        }
    }
    if let Err(e) = writer.finalize() {
        eprintln!("Błąd zapisu: {e}");
        return ExitCode::from(1);
    }
    let m = meters.snapshot();
    eprintln!(
        "OK: {:.1} s audio w {:.2} s ({:.1}% czasu rzeczywistego na 1 rdzeniu), latencja toru {} próbek ({:.1} ms), \
         ost. LUFS(3 s) {:.1}, TP(3 s) {:.2} dBTP",
        dur,
        dt,
        100.0 * dt / dur.max(1e-9),
        engine.latency(),
        1000.0 * engine.latency() as f64 / ctx.fs,
        m.lufs_short,
        m.true_peak_db
    );
    ExitCode::SUCCESS
}
