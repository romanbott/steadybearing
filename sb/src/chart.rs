pub fn decimate(values: &[f32], width: usize) -> Vec<f32> {
    if values.is_empty() || width == 0 {
        return vec![];
    }

    let bucket_size = (values.len() as f32 / width as f32).ceil() as usize;

    (0..width)
        .map(|i| {
            let start = i * bucket_size;
            let end = ((i + 1) * bucket_size).min(values.len());
            if start >= values.len() {
                return 0.0;
            }
            values[start..end]
                .iter()
                .cloned()
                .fold(f32::MIN, f32::max)
        })
        .collect()
}
