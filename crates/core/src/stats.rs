/// Statistiques descriptives d'une série de valeurs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub n: usize,
    pub sum: f64,
    pub mean: f64,
    /// Écart-type d'échantillon (n-1) ; 0 si n < 2.
    pub sd: f64,
    pub min: f64,
    pub max: f64,
}

impl Stats {
    pub fn of(values: &[f64]) -> Option<Stats> {
        if values.is_empty() {
            return None;
        }
        let n = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / n as f64;
        let sd = if n < 2 {
            0.0
        } else {
            (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
        };
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Some(Stats { n, sum, mean, sd, min, max })
    }
}
