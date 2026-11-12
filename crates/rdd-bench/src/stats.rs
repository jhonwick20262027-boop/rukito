//! Estadísticas de latencia del arnés: percentiles por rango más cercano.
//!
//! Las muestras se guardan en nanosegundos para no perder precisión en
//! operaciones de microsegundos; el informe las publica en milisegundos.

/// Percentil `p` (0.0..=100.0) por rango más cercano; 0 si no hay muestras.
pub fn percentile(samples: &[u64], p: f64) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

pub fn nanos_to_ms(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

pub fn rate(total: u64, elapsed: std::time::Duration) -> f64 {
    if elapsed.is_zero() {
        return 0.0;
    }
    total as f64 / elapsed.as_secs_f64()
}
