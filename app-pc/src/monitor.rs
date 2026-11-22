//! Podsłuch wyjścia (WASAPI loopback) tylko do analizatora widma: gdy gra aplikacja Spotify, przeglądarka itp.,
//! a nie wbudowany odtwarzacz. Działa tylko przy powiększonej karcie odtwarzacza (oszczędność CPU).

use crate::odtwarzacz::{Analizator, PASMA};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct Monitor {
    _stream: cpal::Stream,
    pub urzadzenie: String,
    /// strumień zgłosił błąd (urządzenie znikło) – trzeba otworzyć podsłuch od nowa
    pub martwy: Arc<std::sync::atomic::AtomicBool>,
}

/// `wyjscie` – nazwa urządzenia, na którym słychać dźwięk (pusta = domyślne).
pub fn start(wyjscie: &str, widmo: Arc<Mutex<[f32; PASMA]>>) -> Result<Monitor, String> {
    let host = cpal::default_host();
    let dev = if wyjscie.is_empty() {
        host.default_output_device()
    } else {
        host.output_devices().ok().and_then(|mut it| it.find(|d| d.name().map(|n| n == wyjscie).unwrap_or(false)))
    }
    .ok_or("brak urządzenia do podsłuchu")?;
    let nazwa = dev.name().unwrap_or_default();
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    if cfg.sample_format() != cpal::SampleFormat::F32 {
        return Err("format inny niż f32".into());
    }
    let ch = cfg.channels() as usize;
    let mut an = Analizator::new(cfg.sample_rate().0);
    let mut st = Vec::<f32>::with_capacity(8192);
    let martwy = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let martwy2 = martwy.clone();
    // WASAPI: strumień wejściowy na urządzeniu wyjściowym = loopback
    let stream = dev
        .build_input_stream(
            &cfg.config(),
            move |data: &[f32], _| {
                st.clear();
                for fr in data.chunks(ch) {
                    st.push(fr[0]);
                    st.push(if ch > 1 { fr[1] } else { fr[0] });
                }
                an.dodaj(&st, &widmo);
            },
            move |e| {
                log::warn!("podsłuch widma: {e}");
                martwy2.store(true, std::sync::atomic::Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Monitor { _stream: stream, urzadzenie: nazwa, martwy })
}
