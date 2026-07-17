pub fn decimate(values: &[f32], width: usize) -> Vec<f32> {
    let n = values.len();
    if n == 0 || width == 0 {
        return vec![];
    }

    (0..width)
        .map(|i| {
            let start = ((i as f64 * n as f64) / width as f64).floor() as usize;
            let end = (((i + 1) as f64 * n as f64) / width as f64).ceil() as usize;
            let end = end.max(start + 1).min(n);
            if start >= n {
                return values[n - 1];
            }
            values[start..end]
                .iter()
                .cloned()
                .fold(f32::MIN, f32::max)
        })
        .collect()
}
