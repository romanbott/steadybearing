use chrono::{Local, TimeZone};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use sb_protocol::types::{FAN_MAX_RPM, FAN_MIN_RPM};

pub const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Debug, Clone, Copy)]
pub enum MetricKind {
    Usage,
    Temp,
    Fan,
}

pub fn usage_color(val: f32) -> Color {
    let t = (val / 100.0).clamp(0.0, 1.0);
    Color::Rgb((255.0 * t) as u8, (255.0 * (1.0 - t)) as u8, 0)
}

pub fn temp_color(val: f32) -> Color {
    if val <= 35.0 {
        Color::Green
    } else if val >= 85.0 {
        Color::Red
    } else {
        let t = (val - 35.0) / 50.0;
        Color::Rgb((255.0 * t) as u8, (255.0 * (1.0 - t)) as u8, 0)
    }
}

pub fn fan_color(rpm: u16) -> Color {
    let t = if rpm > FAN_MIN_RPM {
        (rpm as f32 - FAN_MIN_RPM as f32) / (FAN_MAX_RPM as f32 - FAN_MIN_RPM as f32)
    } else {
        0.0
    };
    Color::Rgb(
        (255.0 * (1.0 - t * 0.7)) as u8,
        (255.0 * (1.0 - t)) as u8,
        255,
    )
}

/// Shared bar-chart painter for live mode (no grid).
pub fn paint_bars(
    buckets:   &[f32],
    range_min: f32,
    range_max: f32,
    chart_h:   usize,
    gap_val:   Option<f32>,
    kind:      MetricKind,
) -> Vec<Line<'static>> {
    let range = (range_max - range_min).max(1.0);
    let mut lines = Vec::new();

    for visual_row in (0..chart_h).rev() {
        let mut spans = Vec::new();
        for &val in buckets {
            if let Some(gv) = gap_val {
                if val <= gv + 0.1 {
                    spans.push(Span::raw(" "));
                    continue;
                }
            }
            let norm    = ((val - range_min) / range).clamp(0.0, 1.0);
            let bar_top = norm * chart_h as f32;
            let fill    = (bar_top - visual_row as f32).clamp(0.0, 1.0);
            let ch      = BLOCKS[(fill * 8.0).round() as usize];
            let color = match kind {
                MetricKind::Usage => usage_color(val),
                MetricKind::Temp  => temp_color(val),
                MetricKind::Fan   => fan_color(val as u16),
            };
            spans.push(Span::styled(ch.to_string(), ratatui::style::Style::default().fg(color)));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Pixel-buffer painter with grid (live mode).
/// `timestamps` are the epoch-second timestamps of the original source
/// samples, indexed in the same order as the values that produced `buckets`.
pub fn paint_bars_grid(
    buckets:     &[f32],
    range_min:   f32,
    range_max:   f32,
    chart_h:     usize,
    kind:        MetricKind,
    timestamps:  &[i64],
) -> Vec<Line<'static>> {
    let chart_w = buckets.len();
    let buf_h = chart_h;
    let mut buf: Vec<Vec<(char, Style)>> = vec![vec![(' ', Style::default()); chart_w]; buf_h];

    // ── grid ─────────────────────────────────────────────────
    let gs = Style::default().fg(Color::DarkGray);
    let mid = chart_h / 2;
    let q3 = chart_h * 3 / 4;
    let q1 = chart_h / 4;

    // horizontal grid lines
    for row in 0..buf_h {
        if row == mid || (chart_h >= 8 && (row == q1 || row == q3)) {
            for col in 0..chart_w {
                buf[row][col] = ('─', gs);
            }
        }
    }

    // vertical grid lines + labels at minute marks
    let n = timestamps.len();
    let ls = Style::default().fg(Color::White);
    for i in 1..n {
        if timestamps[i] / 60 != timestamps[i - 1] / 60 {
            let col = (i * chart_w) / n;
            if col >= chart_w { continue; }
            // vertical line
            for row in 0..buf_h {
                if row == mid || row == q1 || row == q3 {
                    buf[row][col] = ('┼', gs);
                } else {
                    buf[row][col] = ('│', gs);
                }
            }
            // label
            let label = Local.timestamp_opt(timestamps[i], 0)
                .single()
                .map(|d| d.format("%H:%M").to_string())
                .unwrap_or_default();
            let sx = col.saturating_sub(label.len() / 2);
            for (j, ch) in label.chars().enumerate() {
                let x = sx + j;
                if x < chart_w { buf[0][x] = (ch, ls); }
            }
        }
    }

    // horizontal labels
    let mid_val = (range_min + range_max) / 2.0;
    let q1_val = range_min + (range_max - range_min) * 0.25;
    let q3_val = range_min + (range_max - range_min) * 0.75;
    for (row, val) in [(mid, mid_val), (q1, q3_val), (q3, q1_val)] {
        let text = match kind {
            MetricKind::Usage => format!("{:.0}%", val),
            MetricKind::Temp  => format!("{:.0}°C", val),
            MetricKind::Fan   => format!("{}RPM", val as u16),
        };
        let lx = 0;
        for (i, ch) in text.chars().enumerate() {
            let x = lx + i;
            if x < chart_w { buf[row][x] = (ch, ls); }
        }
    }

    // ── bars (overwrite grid) ────────────────────────────────
    let range = (range_max - range_min).max(1.0);
    for visual_row in 0..chart_h {
        let y = chart_h - 1 - visual_row;
        for (x, &val) in buckets.iter().enumerate() {
            let norm    = ((val - range_min) / range).clamp(0.0, 1.0);
            let bar_top = norm * chart_h as f32;
            let fill    = (bar_top - visual_row as f32).clamp(0.0, 1.0);
            let ch      = BLOCKS[(fill * 8.0).round() as usize];
            if ch != ' ' {
                let color = match kind {
                    MetricKind::Usage => usage_color(val),
                    MetricKind::Temp  => temp_color(val),
                    MetricKind::Fan   => fan_color(val as u16),
                };
                buf[y][x] = (ch, Style::default().fg(color));
            }
        }
    }

    // ── convert to lines ─────────────────────────────────────
    buf.into_iter()
        .map(|row| {
            Line::from(
                row.into_iter()
                    .map(|(ch, s)| Span::styled(ch.to_string(), s))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}
