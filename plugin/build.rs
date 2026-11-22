// Znacznik budowy do dziennika diagnostycznego (czas budowy, UTC, sekundy od epoki).
fn main() {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=SZKLO_BUILD_TAG={t}");
    println!("cargo:rerun-if-changed=src");
}
