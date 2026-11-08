// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reducer {
    Max,
    Min,
    Mean,
}

/// Pool values down to `width` columns, keeping the maximum of each bucket.
pub fn decimate(values: &[f32], width: usize) -> Vec<f32> {
    decimate_with(values, width, Reducer::Max)
}

pub fn decimate_with(values: &[f32], width: usize, reducer: Reducer) -> Vec<f32> {
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
            let bucket = &values[start..end];
            match reducer {
                Reducer::Max => bucket.iter().copied().fold(f32::MIN, f32::max),
                Reducer::Min => bucket.iter().copied().fold(f32::MAX, f32::min),
                Reducer::Mean => bucket.iter().sum::<f32>() / bucket.len() as f32,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{decimate, decimate_with, Reducer};

    #[test]
    fn max_pools_each_bucket() {
        assert_eq!(decimate_with(&[1.0, 3.0, 2.0, 4.0], 2, Reducer::Max), vec![3.0, 4.0]);
    }

    #[test]
    fn min_pools_each_bucket() {
        assert_eq!(decimate_with(&[1.0, 3.0, 2.0, 4.0], 2, Reducer::Min), vec![1.0, 2.0]);
    }

    #[test]
    fn mean_pools_each_bucket() {
        assert_eq!(decimate_with(&[1.0, 3.0, 2.0, 4.0], 2, Reducer::Mean), vec![2.0, 3.0]);
    }

    #[test]
    fn decimate_defaults_to_max() {
        assert_eq!(decimate(&[1.0, 3.0, 2.0, 4.0], 2), vec![3.0, 4.0]);
    }

    #[test]
    fn empty_input_and_zero_width_yield_nothing() {
        assert!(decimate_with(&[], 4, Reducer::Mean).is_empty());
        assert!(decimate_with(&[1.0], 0, Reducer::Mean).is_empty());
    }
}
