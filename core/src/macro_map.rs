//! Mapowanie suwaka „moc” 0..1 → parametry wewnętrzne (krzywe; do dopasowania w fazie 5).

/// Krzywa wypukła/wklęsła: a^g (g < 1 – szybki przyrost na początku).
#[inline]
pub fn curve(a: f64, g: f64) -> f64 {
    a.clamp(0.0, 1.0).powf(g)
}
/// Interpolacja liniowa zakresu parametru wg mocy.
#[inline]
pub fn range(a: f64, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * a.clamp(0.0, 1.0)
}
/// Interpolacja logarytmiczna (np. czasy, częstotliwości).
#[inline]
pub fn range_log(a: f64, lo: f64, hi: f64) -> f64 {
    lo * (hi / lo).powf(a.clamp(0.0, 1.0))
}
