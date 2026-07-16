use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use sb_protocol::types::{HistoricalRow, MetricSample, Snapshot, FAN_MAX_RPM, FAN_MIN_RPM};

use crate::chart;

const MAX_LIVE_SAMPLES: usize = 3600;
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Debug, Clone, Copy)]
enum DisplayMode {
    Raw,
    MA,
    Min,
    Max,
}

impl DisplayMode {
    fn next(self) -> Self {
        match self {
            DisplayMode::Raw => DisplayMode::MA,
            DisplayMode::MA => DisplayMode::Min,
            DisplayMode::Min => DisplayMode::Max,
            DisplayMode::Max => DisplayMode::Raw,
        }
    }

    fn next_historical(self) -> Self {
        match self {
            DisplayMode::Raw | DisplayMode::MA => DisplayMode::Max,
            DisplayMode::Max => DisplayMode::Min,
            DisplayMode::Min => DisplayMode::MA,
        }
    }

    fn label(self) -> &'static str {
        match self {
            DisplayMode::Raw => "1s raw",
            DisplayMode::MA => "10s MA",
            DisplayMode::Min => "10s min",
            DisplayMode::Max => "10s max",
        }
    }

    fn label_historical(self) -> &'static str {
        match self {
            DisplayMode::Raw | DisplayMode::MA => "mean",
            DisplayMode::Min => "min",
            DisplayMode::Max => "max",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TimeFrame {
    Live,
    Minute,
    Hourly,
}

impl TimeFrame {
    fn next(self) -> Self {
        match self {
            TimeFrame::Live => TimeFrame::Minute,
            TimeFrame::Minute => TimeFrame::Hourly,
            TimeFrame::Hourly => TimeFrame::Live,
        }
    }

    fn label(self) -> &'static str {
        match self {
            TimeFrame::Live => "live",
            TimeFrame::Minute => "1m buckets",
            TimeFrame::Hourly => "1h buckets",
        }
    }
}

pub struct TuiApp {
    socket_path: String,
    live_ticks: VecDeque<MetricSample>,
    historical_data: Vec<HistoricalRow>,
    snapshot: Option<Snapshot>,
    mode: DisplayMode,
    timeframe: TimeFrame,
    scroll_offset: usize,
    selection: usize,
    key_repeat: Option<(KeyCode, u32)>,
}

impl TuiApp {
    pub fn new(socket_path: String) -> Self {
        TuiApp {
            socket_path,
            live_ticks: VecDeque::with_capacity(MAX_LIVE_SAMPLES),
            historical_data: Vec::new(),
            snapshot: None,
            mode: DisplayMode::Raw,
            timeframe: TimeFrame::Live,
            scroll_offset: 0,
            selection: 0,
            key_repeat: None,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        terminal.clear()?;

        let mut framed = match crate::client::connect(&self.socket_path).await {
            Ok(f) => f,
            Err(e) => {
                self.render_disconnected(terminal, &e.to_string())?;
                return Ok(());
            }
        };

        let mut snapshot = match crate::client::request_snapshot(&mut framed).await {
            Ok(s) => s,
            Err(e) => {
                self.render_disconnected(terminal, &e.to_string())?;
                return Ok(());
            }
        };

        let live_samples: VecDeque<MetricSample> =
            std::mem::take(&mut snapshot.tier1).into();
        self.live_ticks = live_samples;
        self.snapshot = Some(snapshot);

        crate::client::request_stream_live(&mut framed).await?;

        let tick_rate = Duration::from_millis(100);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|f| self.render(f))?;

            let timeout = if self.timeframe == TimeFrame::Live {
                tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or(Duration::ZERO)
            } else {
                Duration::from_secs(60)
            };

            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Char('m') => {
                                if self.timeframe == TimeFrame::Live {
                                    self.mode = self.mode.next();
                                } else {
                                    self.mode = self.mode.next_historical();
                                }
                            }
                            KeyCode::Char('t') => {
                                let new = self.timeframe.next();
                                if let Err(e) = self
                                    .switch_timeframe(&mut framed, new)
                                    .await
                                {
                                    self.render_disconnected(
                                        terminal,
                                        &e.to_string(),
                                    )?;
                                    return Ok(());
                                }
                            }
                            KeyCode::Left => {
                                if self.timeframe != TimeFrame::Live {
                                    let step = self.advance_repeat(KeyCode::Left);
                                    self.selection =
                                        self.selection.saturating_sub(step);
                                }
                            }
                            KeyCode::Right => {
                                if self.timeframe != TimeFrame::Live {
                                    let step = self.advance_repeat(KeyCode::Right);
                                    let data_len = self.historical_data.len();
                                    self.selection =
                                        (self.selection + step).min(data_len.saturating_sub(1));
                                }
                            }
                            _ => {
                                self.key_repeat = None;
                            }
                        }
                    }
                }
            }

            if self.timeframe == TimeFrame::Live {
                if last_tick.elapsed() >= tick_rate {
                    match crate::client::read_next_tick(&mut framed).await {
                        Ok(sample) => {
                            if self.live_ticks.len() >= MAX_LIVE_SAMPLES {
                                self.live_ticks.pop_front();
                            }
                            self.live_ticks.push_back(sample);
                        }
                        Err(_) => break,
                    }
                    last_tick = Instant::now();
                }
            }
        }

        ratatui::restore();
        Ok(())
    }

    async fn switch_timeframe(
        &mut self,
        framed: &mut crate::client::IpcFramed,
        new: TimeFrame,
    ) -> anyhow::Result<()> {
        if new == TimeFrame::Live {
            *framed = crate::client::connect(&self.socket_path).await?;

            let mut snapshot = crate::client::request_snapshot(framed).await?;
            let samples: VecDeque<MetricSample> =
                std::mem::take(&mut snapshot.tier1).into();
            self.live_ticks = samples;
            self.snapshot = Some(snapshot);
            self.timeframe = TimeFrame::Live;
            self.scroll_offset = 0;
            self.selection = 0;
            self.key_repeat = None;
            self.historical_data.clear();

            crate::client::request_stream_live(framed).await?;
            return Ok(());
        }

        let bucket_secs = match new {
            TimeFrame::Minute => 60_u32,
            TimeFrame::Hourly => 3600,
            TimeFrame::Live => unreachable!(),
        };

        let count = 2000;

        let mut conn = crate::client::connect(&self.socket_path).await?;
        let rows =
            crate::client::request_historical(&mut conn, bucket_secs, count).await?;

        self.historical_data = rows;
        self.timeframe = new;
        self.scroll_offset = 0;
        self.selection = self.historical_data.len().saturating_sub(1);
        self.key_repeat = None;
        Ok(())
    }

    fn arrow_step(count: u32) -> usize {
        match count {
            1..=8 => 1,
            9..=25 => 2,
            26..=60 => 5,
            _ => 8,
        }
    }

    fn advance_repeat(&mut self, code: KeyCode) -> usize {
        let count = match self.key_repeat {
            Some((prev, c)) if prev == code => c + 1,
            _ => 1,
        };
        self.key_repeat = Some((code, count));
        Self::arrow_step(count)
    }

    fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Length(1),
            ])
            .split(f.area());

        let latest = if self.timeframe == TimeFrame::Live {
            self.live_ticks.back().copied()
        } else {
            self.historical_data.last().map(|r| MetricSample {
                timestamp: r.timestamp,
                cpu_temp: r.temp_mean,
                cpu_usage: r.usage_mean,
                fan_rpm: r.rpm_mean as u16,
                pwm_value: r.pwm_mean as u8,
            })
        };

        let (time_str, hist_info) = if self.timeframe != TimeFrame::Live {
            let chart_w = chunks[0].width.saturating_sub(2) as usize;
            let data_len = self.historical_data.len();
            let max_off = data_len.saturating_sub(chart_w);
            self.scroll_offset = self.scroll_offset.min(max_off);
            self.selection = self.selection.min(data_len.saturating_sub(1));

            let margin = (chart_w / 10).max(1);

            let scroll = self.scroll_offset;
            let vp_start = max_off.saturating_sub(scroll);
            let left_gap = margin.saturating_sub(vp_start);
            let right_gap = margin.saturating_sub(scroll);
            let bar_count = chart_w.saturating_sub(left_gap).saturating_sub(right_gap);
            let bar_vp_end = (vp_start + bar_count).min(data_len);

            // position viewport so selection is inside the bar area
            if self.selection >= bar_vp_end {
                let overshoot = self.selection.saturating_sub(bar_vp_end.saturating_sub(1));
                self.scroll_offset = scroll.saturating_sub(overshoot);
            } else if self.selection < vp_start {
                let undershoot = vp_start.saturating_sub(self.selection);
                self.scroll_offset = (scroll + undershoot).min(max_off);
            }

            let vp_start = max_off.saturating_sub(self.scroll_offset);
            let left_gap = margin.saturating_sub(vp_start);
            let right_gap = margin.saturating_sub(self.scroll_offset);

            let ts = self.historical_data.get(self.selection).map(|r| r.timestamp);
            let time = ts.map(|t| {
                Local.timestamp_opt(t, 0).single().map(|dt| dt.format("%H:%M").to_string()).unwrap_or_default()
            }).unwrap_or_default();
            let display = format!("{time}  ← {}/{max_off} →", self.scroll_offset);
            (Some(display), Some((left_gap, right_gap, vp_start, max_off)))
        } else {
            (None, None)
        };

        let sel_usage = if self.timeframe != TimeFrame::Live {
            self.historical_data.get(self.selection).map(|r| format!("{:.1}%", self.historical_col(r, MetricKind::Usage)))
        } else { None };
        let sel_temp = if self.timeframe != TimeFrame::Live {
            self.historical_data.get(self.selection).map(|r| format!("{:.1}°C", self.historical_col(r, MetricKind::Temp)))
        } else { None };
        let sel_fan = if self.timeframe != TimeFrame::Live {
            self.historical_data.get(self.selection).map(|r| {
                let pwm_pct = (r.pwm_mean / 255.0 * 100.0) as u8;
                format!("{} RPM ({}%)", r.rpm_mean as u16, pwm_pct)
            })
        } else { None };

        self.render_metric_pane(f, chunks[0], "CPU Usage", latest, hist_info, sel_usage, MetricKind::Usage);
        self.render_metric_pane(f, chunks[1], "CPU Temp", latest, hist_info, sel_temp, MetricKind::Temp);
        self.render_metric_pane(f, chunks[2], "Fan", latest, hist_info, sel_fan, MetricKind::Fan);

        let scroll = time_str.unwrap_or_default();

        let mode_label = if self.timeframe == TimeFrame::Live {
            self.mode.label()
        } else {
            self.mode.label_historical()
        };

        let line = Line::from(format!(
            " m: {}  t: {}{}  q: quit",
            mode_label,
            self.timeframe.label(),
            scroll,
        ));
        f.render_widget(line, chunks[3]);
    }

    fn render_metric_pane(
        &self,
        f: &mut Frame,
        area: Rect,
        label: &str,
        latest: Option<MetricSample>,
        hist_info: Option<(usize, usize, usize, usize)>,
        sel_value: Option<String>,
        kind: MetricKind,
    ) {
        if let Some((left_gap, right_gap, vp_start, max_off)) = hist_info {
            self.render_historical_pane(f, area, label, kind, sel_value, left_gap, right_gap, vp_start, max_off);
            return;
        }

        let Some(s) = latest else { return };

        let dummy_block = Block::default().borders(Borders::ALL);
        let inner = dummy_block.inner(area);
        let chart_w = inner.width.max(1) as usize;
        let chart_h = inner.height.max(1) as usize;

        let (cur_val, values, range_min, range_max) = self.extract_bars(chart_w, &s, kind);

        let title = match kind {
            MetricKind::Usage => format!("{}: {:.1}%", label, cur_val),
            MetricKind::Temp => format!("{}: {:.1}°C", label, cur_val),
            MetricKind::Fan => {
                let pwm_pct = (s.pwm_value as f32 / 255.0 * 100.0) as u8;
                format!("{}: {} RPM ({}%)", label, cur_val as u16, pwm_pct)
            }
        };
        let block = Block::default().title(title).borders(Borders::ALL);

        let buckets = chart::decimate(&values, chart_w);

        let mut lines = Vec::new();
        for visual_row in (0..chart_h).rev() {
            let mut spans = Vec::new();
            for &val in &buckets {
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
            lines.push(Line::from(spans));
        }

        let para = Paragraph::new(lines).block(block);
        f.render_widget(para, area);
    }

    fn render_historical_pane(
        &self,
        f: &mut Frame,
        area: Rect,
        label: &str,
        kind: MetricKind,
        sel_value: Option<String>,
        left_gap: usize,
        right_gap: usize,
        vp_start: usize,
        _max_off: usize,
    ) {
        if self.historical_data.is_empty() {
            return;
        }

        let dummy_block = Block::default().borders(Borders::ALL);
        let inner = dummy_block.inner(area);
        let chart_w = inner.width.max(1) as usize;
        let caret_h = 1;
        let chart_h = inner.height.max(1).saturating_sub(caret_h) as usize;

        let bar_count = chart_w.saturating_sub(left_gap).saturating_sub(right_gap);
        let vp_end = (vp_start + bar_count).min(self.historical_data.len());
        let bar_count = (vp_end.saturating_sub(vp_start)).min(bar_count);
        let data_slice = &self.historical_data[vp_start..vp_end];

        let bar_vals: Vec<f32> = data_slice.iter().map(|r| self.historical_col(r, kind)).collect();
        let (range_min, range_max) = if bar_vals.is_empty() {
            (0.0, 1.0)
        } else {
            let min = bar_vals.iter().cloned().fold(f32::MAX, f32::min);
            let max = bar_vals.iter().cloned().fold(f32::MIN, f32::max);
            (min, if max > min { max } else { min + 1.0 })
        };

        let decimated = if bar_count > 0 && !bar_vals.is_empty() {
            chart::decimate(&bar_vals, bar_count)
        } else {
            vec![]
        };
        let dec_len = decimated.len();

        let gap_val = range_min - (range_max - range_min).max(1.0);
        let mut buckets = Vec::with_capacity(chart_w);
        for _ in 0..left_gap { buckets.push(gap_val); }
        buckets.extend(decimated);
        for _ in 0..right_gap { buckets.push(gap_val); }

        let vis_col = if self.selection >= vp_start && self.selection < vp_end {
            let col_in_raw = self.selection - vp_start;
            let col_in_bar = if bar_vals.is_empty() { 0 } else {
                (col_in_raw as f32 * bar_count as f32 / bar_vals.len() as f32) as usize
            };
            (left_gap + col_in_bar).min(chart_w.saturating_sub(1))
        } else if self.selection >= vp_end {
            (left_gap + bar_count.saturating_sub(1)).min(chart_w.saturating_sub(1))
        } else {
            left_gap
        };

        let title = if let Some(ref sv) = sel_value {
            format!("{}: {}", label, sv)
        } else {
            format!("{}: --", label)
        };
        let block = Block::default().title(title).borders(Borders::ALL);

        let mut lines = Vec::new();

        // Caret row
        {
            let mut caret_spans = Vec::new();
            for i in 0..chart_w {
                if i == vis_col {
                    let color = if i >= left_gap && i < left_gap + dec_len {
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

        // Bar rows
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

        let para = Paragraph::new(lines).block(block);
        f.render_widget(para, area);
    }

    fn extract_bars(
        &self,
        chart_w: usize,
        latest: &MetricSample,
        kind: MetricKind,
    ) -> (f32, Vec<f32>, f32, f32) {
        match kind {
            MetricKind::Usage => {
                let cur = self.current_val(latest, |t| t.cpu_usage, 0.0);
                let vals = self.build_values(chart_w, |t| t.cpu_usage);
                (cur, vals, 0.0, 100.0)
            }
            MetricKind::Temp => {
                let cur = self.current_val(latest, |t| t.cpu_temp, 0.0);
                let vals = self.build_values(chart_w, |t| t.cpu_temp);
                (cur, vals, 0.0, 100.0)
            }
            MetricKind::Fan => {
                let cur = latest.fan_rpm;
                let vals = self.build_values(chart_w, |t| t.fan_rpm as f32);
                let floor = FAN_MIN_RPM as f32 * 0.9;
                (cur as f32, vals, floor, FAN_MAX_RPM as f32)
            }
        }
    }

    fn current_val<F>(&self, latest: &MetricSample, extract: F, _default: f32) -> f32
    where
        F: Fn(&MetricSample) -> f32,
    {
        match self.mode {
            DisplayMode::Raw => extract(latest),
            _ => self.window_10s_at(self.live_ticks.len().saturating_sub(1), &extract),
        }
    }

    fn build_values<F>(&self, chart_w: usize, extract: F) -> Vec<f32>
    where
        F: Fn(&MetricSample) -> f32,
    {
        let all: Vec<f32> = (0..self.live_ticks.len())
            .map(|i| {
                if matches!(self.mode, DisplayMode::Raw) || self.timeframe != TimeFrame::Live {
                    extract(&self.live_ticks[i])
                } else {
                    self.window_10s_at(i, &extract)
                }
            })
            .collect();

        let len = all.len();
        if len <= chart_w || self.timeframe == TimeFrame::Live {
            let start = len.saturating_sub(chart_w);
            let end = (start + chart_w).min(len);
            return all[start..end].to_vec();
        }

        let max_off = len - chart_w;
        let offset = self.scroll_offset.min(max_off);
        let start = max_off - offset;
        all[start..start + chart_w].to_vec()
    }

    fn window_10s_at<F>(&self, idx: usize, extract: &F) -> f32
    where
        F: Fn(&MetricSample) -> f32,
    {
        let window = 10;
        let start = idx.saturating_sub(window - 1);
        let count = (idx - start + 1).min(window);
        if count == 0 {
            return 0.0;
        }

        let iter = self
            .live_ticks
            .iter()
            .skip(start)
            .take(count)
            .map(|s| extract(s));

        match self.mode {
            DisplayMode::MA => {
                let sum: f32 = iter.sum();
                sum / count as f32
            }
            DisplayMode::Min => iter.fold(f32::MAX, f32::min),
            DisplayMode::Max => iter.fold(f32::MIN, f32::max),
            DisplayMode::Raw => unreachable!(),
        }
    }

    fn historical_col(&self, row: &HistoricalRow, kind: MetricKind) -> f32 {
        match (self.mode, kind) {
            (DisplayMode::Max, MetricKind::Usage) => row.usage_max,
            (DisplayMode::Min, MetricKind::Usage) => row.usage_min,
            (_, MetricKind::Usage) => row.usage_mean,
            (DisplayMode::Max, MetricKind::Temp) => row.temp_max,
            (DisplayMode::Min, MetricKind::Temp) => row.temp_min,
            (_, MetricKind::Temp) => row.temp_mean,
            (_, MetricKind::Fan) => row.rpm_mean,
        }
    }

    fn render_disconnected(
        &self,
        mut terminal: DefaultTerminal,
        err: &str,
    ) -> anyhow::Result<()> {
        let msg = format!(
            "Cannot connect to sb-daemon.\n\nError: {}\n\nPress any key to exit.",
            err
        );
        terminal.draw(|f| {
            let block = Block::default()
                .title("sb — SteadyBearing")
                .borders(Borders::ALL);
            let text = Paragraph::new(msg).block(block).centered();
            f.render_widget(text, f.area());
        })?;

        crossterm::event::read()?;
        ratatui::restore();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum MetricKind {
    Usage,
    Temp,
    Fan,
}

fn usage_color(val: f32) -> Color {
    let t = (val / 100.0).clamp(0.0, 1.0);
    Color::Rgb((255.0 * t) as u8, (255.0 * (1.0 - t)) as u8, 0)
}

fn temp_color(val: f32) -> Color {
    if val <= 35.0 {
        Color::Green
    } else if val >= 85.0 {
        Color::Red
    } else {
        let t = (val - 35.0) / 50.0;
        Color::Rgb((255.0 * t) as u8, (255.0 * (1.0 - t)) as u8, 0)
    }
}

fn fan_color(rpm: u16) -> Color {
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
