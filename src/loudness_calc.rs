pub fn calculate_weighted_loudness(magnitudes: &Vec<f32>, sample_rate: i32) -> f32 {
    let mut sum = 0.0;
    let mut weighted_sum = 0.0;
    for (i, mag) in magnitudes.iter().enumerate() {
        let freq = i as f32 * sample_rate as f32 / magnitudes.len() as f32 * 2.0;
        let mut weight = 1.0;
        if freq > 100.0 && freq < 8000.0 {
            weight = 1.0 + (freq / 1000.0).log10();
        }
        weighted_sum += mag * mag * weight;
        sum += weight;
    }
    (weighted_sum / sum).sqrt()
}

pub fn calculate_time_domain_loudness(data: &Vec<f32>) -> f32 {
    let mut sum = 0.0;
    for mag in data.iter() {
        sum += (mag.abs() * 100.0).log10();
    };
    sum / data.len() as f32
}