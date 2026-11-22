//! Most JNI: klasa Kotlin `pl.szklo.core.NativeEngine`.
//! `unsafe` tylko na granicy FFI (uchwyt = wskaźnik na `Handle` w Box).

use jni::objects::{JClass, JFloatArray, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use std::sync::{Arc, Mutex};
use szklo_core::{Engine, SharedMeters, SharedParams};

mod diag;
mod spotify;

/// Ostatnie linie dziennika Spotify (do pokazania na karcie i skopiowania).
#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSpotifyLog<'a>(env: JNIEnv<'a>, _c: JClass<'a>) -> JString<'a> {
    env.new_string(diag::tekst()).unwrap_or_default()
}

fn jstr(env: &mut JNIEnv, s: &JString) -> Option<String> {
    env.get_string(s).ok().map(|v| v.into())
}

/// Przekazuje JavaVM i kontekst aplikacji do `ndk-context` (wymaga go cpal/AAudio do otwarcia wyjścia audio).
/// Bez tego `default_output_device()` panikuje w wątku odtwarzacza i nic nie gra. Wołane raz, przed startem Spotify.
#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeInitAndroid(env: JNIEnv, _c: JClass, context: jni::objects::JObject) {
    #[cfg(target_os = "android")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let (Ok(vm), Ok(ctx)) = (env.get_java_vm(), env.new_global_ref(&context)) else { return };
            let ctx_raw = ctx.as_obj().as_raw();
            std::mem::forget(ctx); // globalna referencja żyje do końca procesu
            // SAFETY: wskaźnik VM i globalna referencja kontekstu są ważne przez cały czas życia procesu.
            unsafe { ndk_context::initialize_android_context(vm.get_java_vm_pointer().cast(), ctx_raw.cast()) };
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = (env, context);
}

/// Start odbiornika Spotify Connect „Szkło” na parametrach i miernikach tego uchwytu.
#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSpotifyStart(mut env: JNIEnv, _c: JClass, h: jlong, cache_dir: JString, name: JString) {
    diag::init();
    let Some(h) = handle(h) else { return };
    let (Some(dir), Some(nm)) = (jstr(&mut env, &cache_dir), jstr(&mut env, &name)) else { return };
    spotify::start(h.params.clone(), h.meters.clone(), dir, nm);
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSpotifyStop(_e: JNIEnv, _c: JClass) {
    spotify::stop();
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSpotifyLogout(mut env: JNIEnv, _c: JClass, cache_dir: JString) {
    if let Some(dir) = jstr(&mut env, &cache_dir) {
        spotify::logout(dir);
    }
}

/// Stan: `idle | connecting | login <url> | ready | playing | error <opis>`.
#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSpotifyStatus<'a>(env: JNIEnv<'a>, _c: JClass<'a>) -> JString<'a> {
    env.new_string(spotify::status()).unwrap_or_default()
}

pub struct Handle {
    engine: Mutex<Engine>,
    params: Arc<SharedParams>,
    meters: Arc<SharedMeters>,
    scratch: Mutex<Vec<f32>>,
}

fn handle<'a>(h: jlong) -> Option<&'a Handle> {
    if h == 0 {
        return None;
    }
    // SAFETY: h pochodzi z Box::into_raw w nativeCreate i żyje do nativeDestroy (pilnuje Kotlin).
    Some(unsafe { &*(h as *const Handle) })
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeCreate(_env: JNIEnv, _c: JClass, sample_rate: jint) -> jlong {
    let fs = if sample_rate > 0 { sample_rate as f64 } else { 48_000.0 };
    let (engine, params, meters) = szklo_core::new_engine(fs);
    let h = Box::new(Handle { engine: Mutex::new(engine), params, meters, scratch: Mutex::new(Vec::new()) });
    Box::into_raw(h) as jlong
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeDestroy(_env: JNIEnv, _c: JClass, h: jlong) {
    if h != 0 {
        // SAFETY: zwalniamy dokładnie raz – Kotlin zeruje uchwyt po wywołaniu.
        drop(unsafe { Box::from_raw(h as *mut Handle) });
    }
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSetModule(_e: JNIEnv, _c: JClass, h: jlong, idx: jint, on: jboolean, amount: jfloat) {
    if let Some(h) = handle(h) {
        if idx >= 0 {
            h.params.set_module(idx as usize, on != JNI_FALSE, amount);
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSetMaster(_e: JNIEnv, _c: JClass, h: jlong, m: jfloat) {
    if let Some(h) = handle(h) {
        h.params.set_master(m);
    }
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSetEnabled(_e: JNIEnv, _c: JClass, h: jlong, on: jboolean) {
    if let Some(h) = handle(h) {
        h.params.set_enabled(on != JNI_FALSE);
    }
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSetCompare(_e: JNIEnv, _c: JClass, h: jlong, hold: jboolean) {
    if let Some(h) = handle(h) {
        h.params.set_compare(hold != JNI_FALSE);
    }
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeSetCeiling(_e: JNIEnv, _c: JClass, h: jlong, db: jfloat) {
    if let Some(h) = handle(h) {
        h.params.set_ceiling_dbtp(db);
    }
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeApplyPreset(mut env: JNIEnv, _c: JClass, h: jlong, key: JString) -> jboolean {
    let Some(h) = handle(h) else { return JNI_FALSE };
    let Ok(k) = env.get_string(&key) else { return JNI_FALSE };
    let k: String = k.into();
    match szklo_core::presets::by_key(&k) {
        Some(p) => {
            h.params.apply_preset(p);
            JNI_TRUE
        }
        None => JNI_FALSE,
    }
}

/// Przetwarza w miejscu bufor z przeplotem (frames × channels próbek f32).
#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeProcess(env: JNIEnv, _c: JClass, h: jlong, buf: JFloatArray, frames: jint, channels: jint) {
    let Some(h) = handle(h) else { return };
    if frames <= 0 || channels <= 0 {
        return;
    }
    let n = (frames as usize) * (channels as usize);
    let Ok(len) = env.get_array_length(&buf) else { return };
    if (len as usize) < n {
        return;
    }
    let mut scratch = h.scratch.lock().unwrap_or_else(|e| e.into_inner());
    scratch.resize(n, 0.0);
    if env.get_float_array_region(&buf, 0, &mut scratch[..n]).is_err() {
        return;
    }
    if let Ok(mut e) = h.engine.lock() {
        e.process_interleaved(&mut scratch[..n], channels as usize);
    }
    let _ = env.set_float_array_region(&buf, 0, &scratch[..n]);
}

/// out[0..5] = WE dBFS, WY dBFS, GR dB, LUFS (3 s), dBTP (szczyt 3 s).
#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeMeters(env: JNIEnv, _c: JClass, h: jlong, out: JFloatArray) {
    let Some(h) = handle(h) else { return };
    let s = h.meters.snapshot();
    let v = [s.in_peak_db, s.out_peak_db, s.gr_db, s.lufs_short, s.true_peak_db];
    let _ = env.set_float_array_region(&out, 0, &v);
}

#[no_mangle]
pub extern "system" fn Java_pl_szklo_core_NativeEngine_nativeLatency(_e: JNIEnv, _c: JClass, h: jlong) -> jint {
    handle(h).and_then(|h| h.engine.lock().ok().map(|e| e.latency() as jint)).unwrap_or(0)
}
