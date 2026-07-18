use ratatui::style::Color;
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

/// Shared bar-chart painter used by both live and historical panes.
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
