use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use crate::SharedBuffer;

pub fn time_domain_to_frequency_domain(data: SharedBuffer, planner: &mut FftPlanner<f32>) {
    let guard = data.lock().unwrap();
    let mut local_copy = Vec::new();
    local_copy.clone_from(&*guard);
    drop(guard);
    let n = local_copy.len();
    let fft = planner.plan_fft_forward(n);

    let mut buffer: Vec<Complex<f32>> = local_copy
        .iter()
        .map(|&x| Complex { re: x, im: 0.0 })
        .collect();

    fft.process(&mut buffer);
    let unique_bins = buffer.len() / 2;

    *data.lock().unwrap() = buffer.into_iter()
        .take(unique_bins)
        .map(|c| c.norm())
        .collect();
}