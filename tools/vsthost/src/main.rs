//! szklo-vsthost <szklo_vst2.dll> <we.wav> <wy.wav> [--pipe]
//!
//! Test dymny wtyczki bez Equalizer APO: ładuje DLL jak host VST2 (blok 480 próbek, jak APO przy 48 kHz/10 ms),
//! przetwarza plik, sprawdza parametry, chunk stanu i (z --pipe oraz SZKLO_PIPE_ANYWHERE=1) kanał sterowania
//! named pipe: odbiór mierników i zmianę parametru z „aplikacji”.
#![allow(deprecated)]

use std::io::{BufRead, BufReader, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use vst::host::{Host, HostBuffer, PluginLoader};
use vst::plugin::Plugin;

struct TestHost;
impl Host for TestHost {
    fn automate(&self, _index: i32, _value: f32) {}
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("BŁĄD: {msg}");
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if pos.len() < 3 {
        eprintln!("Użycie: szklo-vsthost <szklo_vst2.dll> <we.wav> <wy.wav> [--pipe]");
        return ExitCode::from(2);
    }
    let test_pipe = args.iter().any(|a| a == "--pipe");
    let host = Arc::new(Mutex::new(TestHost));
    let mut loader = match PluginLoader::load(std::path::Path::new(pos[0].as_str()), host.clone()) {
        Ok(l) => l,
        Err(e) => return fail(&format!("ładowanie DLL: {e:?}")),
    };
    let mut inst = match loader.instance() {
        Ok(i) => i,
        Err(e) => return fail(&format!("instancja: {e:?}")),
    };
    inst.init();
    let info = inst.get_info();
    println!("wtyczka: {} ({}), parametrów {}, wejść {}, wyjść {}, chunki {}", info.name, info.vendor, info.parameters, info.inputs, info.outputs, info.preset_chunks);
    if info.parameters != 48 || info.inputs != 2 || info.outputs != 2 {
        return fail("zła liczba parametrów / kanałów");
    }

    let mut reader = match hound::WavReader::open(pos[1].as_str()) {
        Ok(r) => r,
        Err(e) => return fail(&format!("WAV: {e}")),
    };
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let sc = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap_or(0) as f32 * sc).collect()
        }
    };
    let frames = samples.len() / ch;
    inst.set_sample_rate(spec.sample_rate as f32);
    let block = 480usize;
    inst.set_block_size(block as i64);
    inst.resume();
    let delay = inst.get_info().initial_delay;
    println!("fs {} Hz, zgłoszone opóźnienie {} próbek", spec.sample_rate, delay);

    let params = inst.get_parameter_object();
    for i in [0, 1, 44, 45, 46, 47] {
        println!("  param {:>2}: {:<24} = {:.3} {}", i, params.get_parameter_name(i), params.get_parameter(i), params.get_parameter_text(i));
    }
    // chunk: zapis i odczyt (Equalizer APO trzyma stan w config.txt)
    let chunk = params.get_preset_data();
    println!("chunk stanu: {} B: {}", chunk.len(), String::from_utf8_lossy(&chunk[..chunk.len().min(60)]));
    params.load_preset_data(&chunk);

    // potok IPC – „aplikacja” łączy się w osobnym wątku
    let pipe_thread = if test_pipe {
        Some(std::thread::spawn(|| -> Result<String, String> {
            std::thread::sleep(std::time::Duration::from_millis(300));
            // potok serwera w TYM procesie: szklo-ctl-<pid>-…
            let pref = format!("{}-{}-", szklo_core::ipc::PIPE_PREFIX, std::process::id());
            let name = std::fs::read_dir(r"\\.\pipe\")
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .find(|n| n.starts_with(&pref))
                .ok_or("brak potoku wtyczki")?;
            let f = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\{name}")).map_err(|e| format!("otwarcie potoku: {e}"))?;
            let mut br = BufReader::new(f);
            let mut line = String::new();
            br.read_line(&mut line).map_err(|e| e.to_string())?;
            let m = szklo_core::ipc::decode_meters(&line).ok_or("zła linia mierników")?;
            // ustaw: Hard Limit (idx 22) moc 0,25 i Szerokość (17) wyłączona
            let p = szklo_core::SharedParams::default();
            p.apply_preset(&szklo_core::PRESETS[0]);
            p.set_module(22, true, 0.25);
            p.set_module_on(17, false);
            br.get_mut().write_all(szklo_core::ipc::encode_params(&p).as_bytes()).map_err(|e| e.to_string())?;
            br.get_mut().flush().map_err(|e| e.to_string())?;
            line.clear();
            br.read_line(&mut line).map_err(|e| e.to_string())?;
            Ok(format!("mierniki z wtyczki: fs {} Hz, latencja {}, WY {:.1} dBFS", m.fs, m.latency, m.snap.out_peak_db))
        }))
    } else {
        None
    };

    let mut out = vec![0f32; frames * 2];
    let mut hb: HostBuffer<f32> = HostBuffer::new(2, 2);
    let mut inb = vec![vec![0f32; block]; 2];
    let mut outb = vec![vec![0f32; block]; 2];
    let t0 = std::time::Instant::now();
    let mut off = 0;
    while off < frames {
        let n = (frames - off).min(block);
        for c in 0..2 {
            inb[c].resize(n, 0.0);
            outb[c].resize(n, 0.0);
            for i in 0..n {
                inb[c][i] = samples[(off + i) * ch + if ch > 1 { c } else { 0 }];
            }
        }
        {
            let mut ab = hb.bind(&inb, &mut outb);
            inst.process(&mut ab);
        }
        for i in 0..n {
            out[(off + i) * 2] = outb[0][i];
            out[(off + i) * 2 + 1] = outb[1][i];
        }
        off += n;
        if test_pipe && off < 48_000 * 2 {
            // pierwsze 2 s w tempie zbliżonym do rzeczywistego, żeby potok zdążył się połączyć
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let dt = t0.elapsed().as_secs_f64();
    if let Some(t) = pipe_thread {
        match t.join() {
            Ok(Ok(s)) => println!("potok: OK – {s}"),
            Ok(Err(e)) => return fail(&format!("potok: {e}")),
            Err(_) => return fail("potok: panika wątku"),
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let hl = params.get_parameter(45);
        let w = params.get_parameter(34);
        println!("po zmianie z potoku: Hard Limit moc = {hl:.2} (oczekiwane 0,25), Szerokość wł. = {w} (oczekiwane 0)");
        if (hl - 0.25).abs() > 1e-3 || w != 0.0 {
            return fail("parametry z potoku nie dotarły do wtyczki");
        }
    }
    if out.iter().any(|v| !v.is_finite()) {
        return fail("NaN/Inf w wyjściu");
    }
    let wspec = hound::WavSpec { channels: 2, sample_rate: spec.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = match hound::WavWriter::create(pos[2].as_str(), wspec) {
        Ok(w) => w,
        Err(e) => return fail(&format!("zapis: {e}")),
    };
    for v in &out {
        let _ = w.write_sample(*v);
    }
    let _ = w.finalize();
    inst.suspend();
    drop(inst);
    println!("OK: {:.1} s audio w {:.2} s ({:.1}% czasu rzeczywistego), wtyczka wyładowana czysto", frames as f64 / spec.sample_rate as f64, dt, 100.0 * dt * spec.sample_rate as f64 / frames as f64);
    ExitCode::SUCCESS
}
