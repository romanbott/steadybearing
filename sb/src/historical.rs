use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use sb_protocol::types::{HistoricalRow, FAN_MAX_RPM, FAN_MIN_RPM};

use crate::chart;

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// ── viewport state ──────────────────────────────────────────────────
//
//  selection    = data index the caret points at
//  desired_col  = user-requested visual column of the caret
//
//  vp_start     = (selection − desired_col).clamp(0, max_off)
//  Everything else follows from vp_start + chart_w.

pub struct HistState {
    pub vp_start: usize,
    pub left_gap: usize,
    pub right_gap: usize,
    pub bar_count: usize,
    pub vp_end: usize,
    pub scroll_offset: usize,
    pub max_off: usize,
}

impl HistState {
    pub fn new(data_len: usize, selection: usize, desired_col: usize, chart_w: usize) -> Self {
        let margin = (chart_w / 10).max(1);
        let max_off = data_len.saturating_sub(chart_w);
        let vp_start = (selection.saturating_sub(desired_col)).min(max_off);
        let scroll = max_off.saturating_sub(vp_start);
        let left_gap = margin.saturating_sub(vp_start);
        let right_gap = margin.saturating_sub(scroll);
        let bar_count = chart_w.saturating_sub(left_gap).saturating_sub(right_gap);
        let data_cnt = chart_w.saturating_sub(left_gap);
        let vp_end = (vp_start + data_cnt).min(data_len);

        // eprintln!(
        //     "HistState: margin={margin} max_off={max_off} chart_w={chart_w} data_len={data_len}\n\
        //      \x20  selection={selection} desired_col={desired_col} vp_start={vp_start}\n\
        //      \x20  scroll={scroll} left_gap={left_gap} right_gap={right_gap} bar_count={bar_count} vp_end={vp_end}"
        // );

        HistState {
            vp_start,
            left_gap,
            right_gap,
            bar_count,
            vp_end,
            scroll_offset: scroll,
            max_off,
        }
    }
}

// ── display mode ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum DisplayMode {
    Raw,
    MA,
    Min,
    Max,
}

impl DisplayMode {
    pub fn next(self) -> Self {
        match self {
            DisplayMode::Raw => DisplayMode::MA,
            DisplayMode::MA => DisplayMode::Min,
            DisplayMode::Min => DisplayMode::Max,
            DisplayMode::Max => DisplayMode::Raw,
        }
    }

    pub fn next_historical(self) -> Self {
        match self {
            DisplayMode::Raw | DisplayMode::MA => DisplayMode::Max,
            DisplayMode::Max => DisplayMode::Min,
            DisplayMode::Min => DisplayMode::MA,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::Raw => "1s raw",
            DisplayMode::MA => "10s MA",
            DisplayMode::Min => "10s min",
            DisplayMode::Max => "10s max",
        }
    }

    pub fn label_historical(self) -> &'static str {
        match self {
            DisplayMode::Raw | DisplayMode::MA => "mean",
            DisplayMode::Min => "min",
            DisplayMode::Max => "max",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MetricKind {
    Usage,
    Temp,
    Fan,
}

// ── colour helpers ───────────────────────────────────────────────────

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

// ── column mapping ───────────────────────────────────────────────────

pub fn historical_col(mode: DisplayMode, row: &HistoricalRow, kind: MetricKind) -> f32 {
    match (mode, kind) {
        (DisplayMode::Max, MetricKind::Usage) => row.usage_max,
        (DisplayMode::Min, MetricKind::Usage) => row.usage_min,
        (_, MetricKind::Usage) => row.usage_mean,
        (DisplayMode::Max, MetricKind::Temp) => row.temp_max,
        (DisplayMode::Min, MetricKind::Temp) => row.temp_min,
        (_, MetricKind::Temp) => row.temp_mean,
        (_, MetricKind::Fan) => row.rpm_mean,
    }
}

// ── rendering ────────────────────────────────────────────────────────

pub fn render_pane(
    f: &mut Frame,
    area: Rect,
    label: &str,
    hs: &HistState,
    caret_col: usize,
    sel_value: Option<String>,
    kind: MetricKind,
    mode: DisplayMode,
    data: &[HistoricalRow],
) {
    if data.is_empty() {
        return;
    }

    let inner = Block::default().borders(Borders::ALL).inner(area);
    let chart_w = inner.width.max(1) as usize;
    let caret_h = 1;
    let chart_h = inner.height.max(1).saturating_sub(caret_h) as usize;

    let data_slice = &data[hs.vp_start..hs.vp_end];
    let bar_vals: Vec<f32> = data_slice
        .iter()
        .map(|r| historical_col(mode, r, kind))
        .collect();

    let (range_min, range_max) = if bar_vals.is_empty() {
        (0.0, 1.0)
    } else {
        let min = bar_vals.iter().fold(f32::MAX, |a, v| a.min(*v));
        let max = bar_vals.iter().fold(f32::MIN, |a, v| a.max(*v));
        (min, if max > min { max } else { min + 1.0 })
    };

    let decimated = if hs.bar_count > 0 && !bar_vals.is_empty() {
        chart::decimate(&bar_vals, hs.bar_count)
    } else {
        vec![]
    };
    let dec_len = decimated.len();

    let gap_val = range_min - (range_max - range_min).max(1.0);
    let mut buckets = Vec::with_capacity(chart_w);
    for _ in 0..hs.left_gap {
        buckets.push(gap_val);
    }
    buckets.extend(decimated);
    for _ in 0..hs.right_gap {
        buckets.push(gap_val);
    }

    let vis_col = caret_col.min(chart_w.saturating_sub(1));

    let title = if let Some(ref sv) = sel_value {
        format!("{label}: {sv}")
    } else {
        format!("{label}: --")
    };
    let block = Block::default().title(title).borders(Borders::ALL);

    // ── caret row ───────────────────────────────────────────────────
    let mut lines = Vec::new();
    {
        let mut caret_spans = Vec::new();
        for i in 0..chart_w {
            if i == vis_col {
                let color = if i >= hs.left_gap && i < hs.left_gap + dec_len {
                    let val = buckets[i];
                    match kind {
                        MetricKind::Usage => usage_color(val),
                        MetricKind::Temp => temp_color(val),
                        MetricKind::Fan => fan_color(val as u16),
                    }
                } else {
                    Color::White
                };
                caret_spans.push(Span::styled("▼", Style::default().fg(color)));
            } else {
                caret_spans.push(Span::raw(" "));
            }
        }
        lines.push(Line::from(caret_spans));
    }

    // ── bar rows ────────────────────────────────────────────────────
    for visual_row in (0..chart_h).rev() {
        let mut spans = Vec::new();
        for &val in &buckets {
            if val <= gap_val + 0.1 {
                spans.push(Span::raw(" "));
            } else {
                let norm = ((val - range_min) / (range_max - range_min)).clamp(0.0, 1.0);
                let bar_top = norm * chart_h as f32;
                let fill = (bar_top - visual_row as f32).clamp(0.0, 1.0);
                let ch = BLOCKS[(fill * 8.0).round() as usize];
                let color = match kind {
                    MetricKind::Usage => usage_color(val),
                    MetricKind::Temp => temp_color(val),
                    MetricKind::Fan => fan_color(val as u16),
                };
                spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
            }
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}
