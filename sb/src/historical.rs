use chrono::{Local, TimeZone};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use sb_protocol::types::HistoricalRow;

use crate::chart;
use crate::render::{self, MetricKind};

// ── viewport state ──────────────────────────────────────────────────
//
//  selection    = data index the caret points at
//  caret_col    = user-requested visual column of the caret
//
//  min        = (selection − caret_col).max(0)
//  max        = (selection + chart_w − caret_col).min(data_len)
//  bar_count  = max − min
//  visual_col = selection − min             (always = caret_col except at boundaries)

pub struct HistState {
    pub min: usize,
    pub max: usize,
    pub bar_count: usize,
    pub visual_col: usize,
    pub scroll_offset: usize,
    pub max_off: usize,
}

impl HistState {
    pub fn new(data_len: usize, selection: usize, caret_col: usize, chart_w: usize) -> Self {
        let min = selection.saturating_sub(caret_col);
        let max = (selection + chart_w.saturating_sub(caret_col)).min(data_len);
        let bar_count = max.saturating_sub(min);
        let visual_col = selection.saturating_sub(min);
        let scroll_offset = min;
        let max_off = data_len.saturating_sub(chart_w);

        HistState {
            min,
            max,
            bar_count,
            visual_col,
            scroll_offset,
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
    grid_secs: Option<u32>,
) {
    if data.is_empty() {
        return;
    }

    let inner = Block::default().borders(Borders::ALL).inner(area);
    let chart_w = inner.width.max(1) as usize;
    let caret_h = 1;
    let chart_h = inner.height.max(1).saturating_sub(caret_h) as usize;
    // No room for a caret row plus at least one bar row; nothing sensible
    // can be laid out in a zero-height pane.
    if chart_h == 0 {
        return;
    }

    let data_slice = &data[hs.min..hs.max];
    let bar_vals: Vec<f32> = data_slice
        .iter()
        .map(|r| historical_col(mode, r, kind))
        .collect();

    let (range_min, range_max) = if bar_vals.is_empty() {
        (0.0, 1.0)
    } else {
        let mn = bar_vals.iter().fold(f32::MAX, |a, v| a.min(*v));
        let mx = bar_vals.iter().fold(f32::MIN, |a, v| a.max(*v));
        (mn, if mx > mn { mx } else { mn + 1.0 })
    };

    let decimated = if hs.bar_count > 0 && !bar_vals.is_empty() {
        chart::decimate(&bar_vals, hs.bar_count)
    } else {
        vec![]
    };
    let dec_len = decimated.len();

    let gap_val = range_min - (range_max - range_min).max(1.0);
    let left_gap = caret_col.saturating_sub(hs.visual_col).min(chart_w);

    let mut buckets = Vec::with_capacity(chart_w);
    for _ in 0..left_gap {
        buckets.push(gap_val);
    }
    buckets.extend(decimated);
    while buckets.len() < chart_w {
        buckets.push(gap_val);
    }

    let title = if let Some(ref sv) = sel_value {
        format!("{label}: {sv}")
    } else {
        format!("{label}: --")
    };
    let block = Block::default().title(title).borders(Borders::ALL);

    // ── pixel buffer ────────────────────────────────────────────────
    let buf_h = chart_h + 1; // caret row + bar rows
    let mut buf: Vec<Vec<(char, Style)>> = vec![vec![(' ', Style::default()); chart_w]; buf_h];

    // ── layer 1: grid ───────────────────────────────────────────────
    let mut major_vlines: Vec<(usize, i64)> = Vec::new();
    let mut minor_vlines: Vec<usize> = Vec::new();
    if let Some(interval) = grid_secs {
        let interval = interval as i64;
        let sub = if interval < 86400 { interval / 2 } else { interval };
        let n = data_slice.len();
        let bc = hs.bar_count;
        for i in 1..n {
            let ts = data_slice[i].timestamp;
            let prev_ts = data_slice[i - 1].timestamp;
            let major = ts / interval != prev_ts / interval;
            let minor = sub != interval && ts / sub != prev_ts / sub;
            if major || minor {
                let col = (i * bc) / n;
                let vis = left_gap + col.min(bc.saturating_sub(1));
                if vis < chart_w {
                    if major {
                        major_vlines.push((vis, ts));
                    } else {
                        minor_vlines.push(vis);
                    }
                }
            }
        }

        let gs = Style::default().fg(Color::DarkGray);
        let mid = chart_h / 2 + 1;
        let q3 = chart_h * 3 / 4 + 1;
        let q1 = chart_h / 4 + 1;

        for row in 0..buf_h {
            let is_line = row == mid || (chart_h >= 8 && (row == q1 || row == q3));
            for col in 0..chart_w {
                let vline = major_vlines.iter().any(|(c, _)| *c == col)
                    || minor_vlines.contains(&col);
                if is_line {
                    buf[row][col] = (if vline { '┼' } else { '─' }, gs);
                } else if vline {
                    buf[row][col] = ('│', gs);
                }
            }
        }
    }

    // ── layer 2: labels ─────────────────────────────────────────────
    if let Some(interval) = grid_secs {
        if !bar_vals.is_empty() {
            let ls = Style::default().fg(Color::White);
            let last_row = buf_h - 1;
            let mid = (chart_h / 2 + 1).min(last_row);
            let q3 = (chart_h * 3 / 4 + 1).min(last_row);
            let q1 = (chart_h / 4 + 1).min(last_row);

            // horizontal labels: mid, q1, q3
            let mid_val = (range_min + range_max) / 2.0;
            let q1_val = range_min + (range_max - range_min) * 0.25;
            let q3_val = range_min + (range_max - range_min) * 0.75;
            for (row, val) in [(mid, mid_val), (q1, q3_val), (q3, q1_val)] {
                let text = match kind {
                    MetricKind::Usage => format!("{:.0}%", val),
                    MetricKind::Temp  => format!("{:.0}°C", val),
                    MetricKind::Fan   => format!("{}RPM", val as u16),
                };
                let lx = left_gap.saturating_sub(text.len());
                for (i, ch) in text.chars().enumerate() {
                    let x = lx + i;
                    if x < chart_w { buf[row][x] = (ch, ls); }
                }
            }

            // vertical labels: major vlines only
            for &(vis, ts) in &major_vlines {
                let v_label = Local.timestamp_opt(ts, 0)
                    .single()
                    .map(|d| {
                        if interval >= 86400 {
                            d.format("%d %b").to_string().to_uppercase()
                        } else {
                            d.format("%H:%M").to_string()
                        }
                    })
                    .unwrap_or_default();
                let sx = vis.saturating_sub(v_label.len() / 2);
                for (i, ch) in v_label.chars().enumerate() {
                    let x = sx + i;
                    if x < chart_w { buf[0][x] = (ch, ls); }
                }
            }
        }
    }

    // ── layer 2: bars (overwrite grid where bar block ≠ space) ──────
    let range = (range_max - range_min).max(1.0);
    for visual_row in 0..chart_h {
        // let y = visual_row + 1; // caret at row 0
        let y = chart_h - visual_row;
        for (x, &val) in buckets.iter().take(chart_w).enumerate() {
            let norm = ((val - range_min) / range).clamp(0.0, 1.0);
            let bar_top = norm * chart_h as f32;
            let fill = (bar_top - visual_row as f32).clamp(0.0, 1.0);
            let ch = render::BLOCKS[(fill * 8.0).round() as usize];
            if ch != ' ' {
                let color = match kind {
                    MetricKind::Usage => render::usage_color(val),
                    MetricKind::Temp => render::temp_color(val),
                    MetricKind::Fan => render::fan_color(val as u16),
                };
                buf[y][x] = (ch, Style::default().fg(color));
            }
        }
    }

    // ── layer 3: caret ──────────────────────────────────────────────
    let vis_col = caret_col.min(chart_w.saturating_sub(1));
    let caret_color = if vis_col >= left_gap && vis_col < left_gap + dec_len {
        let val = buckets.get(vis_col).copied().unwrap_or(gap_val);
        match kind {
            MetricKind::Usage => render::usage_color(val),
            MetricKind::Temp => render::temp_color(val),
            MetricKind::Fan => render::fan_color(val as u16),
        }
    } else {
        Color::White
    };
    buf[0][vis_col] = ('▼', Style::default().fg(caret_color));

    // ── convert buffer to ratatui Lines ──────────────────────────────
    let lines: Vec<Line<'_>> = buf
        .into_iter()
        .map(|row| {
            Line::from(
                row.into_iter()
                    .map(|(ch, s)| Span::styled(ch.to_string(), s))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    f.render_widget(Paragraph::new(lines).block(block), area);
}
