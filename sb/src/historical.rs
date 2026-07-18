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
    pub min:           usize,
    pub max:           usize,
    pub bar_count:     usize,
    pub visual_col:    usize,
    pub scroll_offset: usize,
    pub max_off:       usize,
}

impl HistState {
    pub fn new(
        data_len:    usize,
        selection:   usize,
        caret_col:   usize,
        chart_w:     usize,
    ) -> Self {
        let min          = selection.saturating_sub(caret_col);
        let max          = (selection + chart_w.saturating_sub(caret_col)).min(data_len);
        let bar_count    = max.saturating_sub(min);
        let visual_col   = selection.saturating_sub(min);
        let scroll_offset = min;
        let max_off      = data_len.saturating_sub(chart_w);

        HistState { min, max, bar_count, visual_col, scroll_offset, max_off }
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
        (_,                MetricKind::Usage) => row.usage_mean,
        (DisplayMode::Max, MetricKind::Temp)  => row.temp_max,
        (DisplayMode::Min, MetricKind::Temp)  => row.temp_min,
        (_,                MetricKind::Temp)  => row.temp_mean,
        (_,                MetricKind::Fan)   => row.rpm_mean,
    }
}

// ── rendering ────────────────────────────────────────────────────────

pub fn render_pane(
    f:         &mut Frame,
    area:      Rect,
    label:     &str,
    hs:        &HistState,
    caret_col: usize,
    sel_value:  Option<String>,
    kind:      MetricKind,
    mode:      DisplayMode,
    data:      &[HistoricalRow],
) {
    if data.is_empty() {
        return;
    }

    let inner   = Block::default().borders(Borders::ALL).inner(area);
    let chart_w = inner.width.max(1) as usize;
    let caret_h = 1;
    let chart_h = inner.height.max(1).saturating_sub(caret_h) as usize;

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

    let gap_val  = range_min - (range_max - range_min).max(1.0);
    let left_gap = caret_col.saturating_sub(hs.visual_col);

    let mut buckets = Vec::with_capacity(chart_w);
    for _ in 0..left_gap {
        buckets.push(gap_val);
    }
    buckets.extend(decimated);
    while buckets.len() < chart_w {
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
                let color = if i >= left_gap && i < left_gap + dec_len {
                    let val = buckets[i];
                    match kind {
                        MetricKind::Usage => render::usage_color(val),
                        MetricKind::Temp  => render::temp_color(val),
                        MetricKind::Fan   => render::fan_color(val as u16),
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
    lines.append(&mut render::paint_bars(
        &buckets, range_min, range_max, chart_h, Some(gap_val), kind,
    ));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
