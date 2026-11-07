use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use sb_protocol::types::{HistoricalRow, MetricSample, FAN_MAX_RPM, FAN_MIN_RPM};

use crate::chart;
use crate::historical::{self, DisplayMode, HistState};
use crate::render::{self, MetricKind};

const MAX_LIVE_SAMPLES: usize = 3600;    // 1 hour at 1 Hz
const TICK_RATE_MS: u64 = 100;
const HISTORICAL_POLL_SECS: u64 = 60;
const REPEAT_GRACE_MS: u64 = 500;
const ARROW_SLOW: u32 = 8;       // 1..=8  → step 1
const ARROW_MED: u32 = 25;       // 9..=25 → step 2
const ARROW_FAST: u32 = 60;      // 26..=60 → step 5 (above → step 8)
const PWM_MAX: f32 = 255.0;
const FAN_FLOOR_MULT: f32 = 0.9;
const SMOOTH_WINDOW: usize = 10;

fn historical_margin(chart_w: usize) -> usize {
    (chart_w / 10).max(1)
}

/// Keep the caret inside the middle 80% of the chart. Uses `max`/`min`
/// instead of `usize::clamp` so that very narrow charts (where the lower
/// bound would exceed the upper bound) do not panic.
fn clamp_caret(caret_col: usize, chart_w: usize) -> usize {
    let margin = historical_margin(chart_w);
    let upper = chart_w.saturating_sub(margin + 1);
    caret_col.max(margin).min(upper)
}

/// Keep the same distance from the right edge after a refresh, so a user
/// sitting at the end follows new data while a scrolled user stays put.
fn follow_tail(old_len: usize, selection: usize, new_len: usize) -> usize {
    if new_len == 0 {
        return 0;
    }
    let old_selection = selection.min(old_len.saturating_sub(1));
    let from_end = old_len.saturating_sub(1).saturating_sub(old_selection);
    new_len.saturating_sub(1).saturating_sub(from_end)
}

/// Restores the terminal on every exit path, including early `?` returns.
struct TerminalRestore;

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        ratatui::restore();
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
    historical_data:  Vec<HistoricalRow>,
    mode:            DisplayMode,
    timeframe: TimeFrame,
    selection:       usize,
    chart_w:         usize,
    caret_col:       usize,
    key_repeat: Option<(KeyCode, u32)>,
    last_key_time: Instant,
    show_grid: bool,
    status_msg: Option<String>,
}

impl TuiApp {
    pub fn new(socket_path: String) -> Self {
        TuiApp {
            socket_path,
            live_ticks: VecDeque::with_capacity(MAX_LIVE_SAMPLES),
            historical_data:  Vec::new(),
            mode:            DisplayMode::Raw,
            timeframe: TimeFrame::Live,
            selection:       0,
            chart_w:         0,
            caret_col:       0,
            key_repeat: None,
            last_key_time: Instant::now(),
            show_grid: true,
            status_msg: None,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        let _restore = TerminalRestore;
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

        self.live_ticks = std::mem::take(&mut snapshot.tier1).into();
        crate::client::request_stream_live(&mut framed).await?;

        let tick_rate = Duration::from_millis(TICK_RATE_MS);
        let mut last_tick = Instant::now();
        let mut hist_framed: Option<crate::client::IpcFramed> = None;
        let mut last_hist_poll = Instant::now();

        loop {
            terminal.draw(|f| self.render(f))?;

            let timeout = if self.timeframe == TimeFrame::Live {
                tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or(Duration::ZERO)
            } else {
                Duration::from_secs(HISTORICAL_POLL_SECS)
                    .saturating_sub(last_hist_poll.elapsed())
            };

            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Char('m') => {
                                self.key_repeat = None;
                                if self.timeframe == TimeFrame::Live {
                                    self.mode = self.mode.next();
                                } else {
                                    self.mode = self.mode.next_historical();
                                }
                            }
                            KeyCode::Char('g') => {
                                self.key_repeat = None;
                                self.show_grid = !self.show_grid;
                            }
                            KeyCode::Char('t') => {
                                self.key_repeat = None;
                                let new = self.timeframe.next();
                                match self
                                    .switch_timeframe(&mut framed, &mut hist_framed, new)
                                    .await
                                {
                                    Ok(()) => {
                                        self.selection =
                                            self.historical_data.len().saturating_sub(1);
                                        self.caret_col =
                                            clamp_caret(usize::MAX, self.chart_w);
                                        last_hist_poll = Instant::now();
                                    }
                                    Err(err) => {
                                        self.status_msg =
                                            Some(format!("cannot switch view: {err}"));
                                    }
                                }
                            }
                            KeyCode::Left => {
                                if self.timeframe != TimeFrame::Live {
                                    let step = self.advance_repeat(KeyCode::Left);
                                    self.selection = self.selection.saturating_sub(step);
                                    self.caret_col = clamp_caret(
                                        self.caret_col.saturating_sub(step),
                                        self.chart_w,
                                    );
                                }
                            }
                            KeyCode::Right => {
                                if self.timeframe != TimeFrame::Live {
                                    let step = self.advance_repeat(KeyCode::Right);
                                    let data_len = self.historical_data.len();
                                    self.selection =
                                        (self.selection + step).min(data_len.saturating_sub(1));
                                    self.caret_col = clamp_caret(
                                        self.caret_col.saturating_add(step),
                                        self.chart_w,
                                    );
                                }
                            }
                            _ => {
                                self.key_repeat = None;
                            }
                        }
                    }
                }
            }

            if self.timeframe != TimeFrame::Live
                && last_hist_poll.elapsed() >= Duration::from_secs(HISTORICAL_POLL_SECS)
            {
                if let Err(e) = self.refresh_historical(&mut hist_framed).await {
                    self.status_msg = Some(format!("refresh failed: {e}"));
                }
                last_hist_poll = Instant::now();
            }

            if self.timeframe == TimeFrame::Live && last_tick.elapsed() >= tick_rate {
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

        Ok(())
    }

    async fn switch_timeframe(
        &mut self,
        framed: &mut crate::client::IpcFramed,
        hist_framed: &mut Option<crate::client::IpcFramed>,
        new: TimeFrame,
    ) -> anyhow::Result<()> {
        if new == TimeFrame::Live {
            let mut conn = crate::client::connect(&self.socket_path).await?;
            let mut snap = crate::client::request_snapshot(&mut conn).await?;
            crate::client::request_stream_live(&mut conn).await?;
            *framed = conn;
            *hist_framed = None;
            self.live_ticks = std::mem::take(&mut snap.tier1).into();
            self.historical_data.clear();
            self.timeframe = TimeFrame::Live;
            self.status_msg = None;
            return Ok(());
        }

        let bucket_secs = match new {
            TimeFrame::Minute => 60_u32,
            TimeFrame::Hourly => 3600,
            TimeFrame::Live => unreachable!(),
        };

        if hist_framed.is_none() {
            *hist_framed = Some(crate::client::connect(&self.socket_path).await?);
        }
        let conn = hist_framed.as_mut().expect("historical connection");
        match crate::client::request_historical(conn, bucket_secs, 2000).await {
            Ok(rows) => {
                self.historical_data = rows;
            }
            Err(err) => {
                *hist_framed = None;
                return Err(err);
            }
        }

        self.timeframe = new;
        self.selection = self.historical_data.len().saturating_sub(1);
        self.caret_col = clamp_caret(usize::MAX, self.chart_w);
        self.status_msg = None;
        Ok(())
    }

    async fn refresh_historical(
        &mut self,
        hist_framed: &mut Option<crate::client::IpcFramed>,
    ) -> anyhow::Result<()> {
        let bucket_secs = match self.timeframe {
            TimeFrame::Minute => 60_u32,
            TimeFrame::Hourly => 3600,
            TimeFrame::Live => return Ok(()),
        };

        if hist_framed.is_none() {
            *hist_framed = Some(crate::client::connect(&self.socket_path).await?);
        }
        let conn = hist_framed.as_mut().expect("historical connection");
        let rows = match crate::client::request_historical(conn, bucket_secs, 2000).await {
            Ok(rows) => rows,
            Err(err) => {
                *hist_framed = None;
                return Err(err);
            }
        };

        self.selection = follow_tail(self.historical_data.len(), self.selection, rows.len());
        self.historical_data = rows;
        self.status_msg = None;
        Ok(())
    }

    fn arrow_step(count: u32) -> usize {
        match count {
            1..=ARROW_SLOW => 1,
            _ if count <= ARROW_MED  => 2,
            _ if count <= ARROW_FAST => 5,
            _ => 8,
        }
    }

    fn advance_repeat(&mut self, code: KeyCode) -> usize {
        let now = Instant::now();
        let count = match self.key_repeat {
            Some((prev, c)) if prev == code => {
                if now.duration_since(self.last_key_time) > Duration::from_millis(REPEAT_GRACE_MS) {
                    1
                } else {
                    c + 1
                }
            }
            _ => 1,
        };
        self.key_repeat = Some((code, count));
        self.last_key_time = now;
        Self::arrow_step(count)
    }

    // ── render ──────────────────────────────────────────────────────────

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

        let chart_w = chunks[0].width.saturating_sub(2) as usize;
        self.chart_w = chart_w;
        self.caret_col = clamp_caret(self.caret_col, chart_w);
        if !self.historical_data.is_empty() {
            self.selection = self.selection.min(self.historical_data.len() - 1);
        }

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

        // ── historical ────────────────────────────────────────────────
        let (time_str, hist) = if self.timeframe != TimeFrame::Live {
            let data_len = self.historical_data.len();

            let hs = HistState::new(data_len, self.selection, self.caret_col, self.chart_w);

            let ts = self
                .historical_data
                .get(self.selection)
                .and_then(|r| {
                    Local
                        .timestamp_opt(r.timestamp, 0)
                        .single()
                        .map(|dt| dt.format("%H:%M").to_string())
                })
                .unwrap_or_default();
            let status = format!("{ts}  ← {}/{} →", hs.scroll_offset, hs.max_off);
            (Some(status), Some(hs))
        } else {
            (None, None)
        };

        let sel_usage = self.historical_data.get(self.selection).map(|r| {
            format!(
                "{:.1}%",
                historical::historical_col(self.mode, r, MetricKind::Usage)
            )
        });
        let sel_temp = self.historical_data.get(self.selection).map(|r| {
            format!(
                "{:.1}°C",
                historical::historical_col(self.mode, r, MetricKind::Temp)
            )
        });
        let sel_fan = self.historical_data.get(self.selection).map(|r| {
            let pwm_pct = (r.pwm_mean / PWM_MAX * 100.0) as u8;
            format!("{} RPM ({}%)", r.rpm_mean as u16, pwm_pct)
        });

        let grid_secs = if self.show_grid {
            match self.timeframe {
                TimeFrame::Live => Some(60),
                TimeFrame::Minute => Some(3600),
                TimeFrame::Hourly => Some(86400),
            }
        } else {
            None
        };

        self.render_metric_pane(
            f, chunks[0], "CPU Usage", latest, hist.as_ref(), sel_usage,
            MetricKind::Usage, grid_secs,
        );
        self.render_metric_pane(
            f, chunks[1], "CPU Temp", latest, hist.as_ref(), sel_temp,
            MetricKind::Temp, grid_secs,
        );
        self.render_metric_pane(
            f, chunks[2], "Fan", latest, hist.as_ref(), sel_fan,
            MetricKind::Fan, grid_secs,
        );

        let scroll_status = time_str.unwrap_or_default();
        let mode_label = if self.timeframe == TimeFrame::Live {
            self.mode.label()
        } else {
            self.mode.label_historical()
        };
        let mut footer = format!(
            " m: {}  t: {}  g:{} {}  q: quit",
            mode_label,
            self.timeframe.label(),
            if self.show_grid { "on" } else { "off" },
            scroll_status,
        );
        if let Some(msg) = &self.status_msg {
            footer.push_str("  ");
            footer.push_str(msg);
        }
        f.render_widget(Line::from(footer), chunks[3]);
    }

    fn render_metric_pane(
        &self,
        f: &mut Frame,
        area: Rect,
        label: &str,
        latest: Option<MetricSample>,
        hs: Option<&HistState>,
        sel_value: Option<String>,
        kind: MetricKind,
        grid_secs: Option<u32>,
    ) {
        if let Some(h) = hs {
            historical::render_pane(
                f,
                area,
                label,
                h,
                self.caret_col,
                sel_value,
                kind,
                self.mode,
                &self.historical_data,
                grid_secs,
            );
            return;
        }

        let Some(s) = latest else { return };

        let inner = Block::default().borders(Borders::ALL).inner(area);
        let chart_w = inner.width.max(1) as usize;
        let chart_h = inner.height.max(1) as usize;

        let (cur_val, values, timestamps, range_min, range_max) = self.extract_bars(chart_w, &s, kind);

        let title = match kind {
            MetricKind::Usage => format!("{label}: {:.1}%", cur_val),
            MetricKind::Temp => format!("{label}: {:.1}°C", cur_val),
            MetricKind::Fan => {
                let pwm_pct = (s.pwm_value as f32 / PWM_MAX * 100.0) as u8;
                format!("{label}: {} RPM ({}%)", cur_val as u16, pwm_pct)
            }
        };
        let block = Block::default().title(title).borders(Borders::ALL);
        let buckets = chart::decimate(&values, chart_w);

        let lines = if grid_secs.is_some() {
            render::paint_bars_grid(
                &buckets, range_min, range_max, chart_h, kind,
                &timestamps,
            )
        } else {
            render::paint_bars(&buckets, range_min, range_max, chart_h, None, kind)
        };
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    // ── live data helpers ──────────────────────────────────────────────

    fn extract_bars(
        &self,
        chart_w: usize,
        latest: &MetricSample,
        kind: MetricKind,
    ) -> (f32, Vec<f32>, Vec<i64>, f32, f32) {
        match kind {
            MetricKind::Usage => {
                let cur = self.current_val(latest, |t| t.cpu_usage);
                let (vals, start) = self.build_values(chart_w, |t| t.cpu_usage);
                let tss = self.build_timestamps(start, chart_w);
                (cur, vals, tss, 0.0, 100.0)
            }
            MetricKind::Temp => {
                let cur = self.current_val(latest, |t| t.cpu_temp);
                let (vals, start) = self.build_values(chart_w, |t| t.cpu_temp);
                let tss = self.build_timestamps(start, chart_w);
                (cur, vals, tss, 0.0, 100.0)
            }
            MetricKind::Fan => {
                let cur = latest.fan_rpm;
                let (vals, start) = self.build_values(chart_w, |t| t.fan_rpm as f32);
                let tss = self.build_timestamps(start, chart_w);
                let floor = FAN_MIN_RPM as f32 * FAN_FLOOR_MULT;
                (cur as f32, vals, tss, floor, FAN_MAX_RPM as f32)
            }
        }
    }

    fn current_val<F>(&self, latest: &MetricSample, extract: F) -> f32
    where
        F: Fn(&MetricSample) -> f32,
    {
        match self.mode {
            DisplayMode::Raw => extract(latest),
            _ => self.window_10s_at(self.live_ticks.len().saturating_sub(1), &extract),
        }
    }

    fn build_values<F>(&self, chart_w: usize, extract: F) -> (Vec<f32>, usize)
    where
        F: Fn(&MetricSample) -> f32,
    {
        let all: Vec<f32> = (0..self.live_ticks.len())
            .map(|i| {
                if matches!(self.mode, DisplayMode::Raw) {
                    extract(&self.live_ticks[i])
                } else {
                    self.window_10s_at(i, &extract)
                }
            })
            .collect();
        let len = all.len();
        let start = len.saturating_sub(chart_w);
        let end = (start + chart_w).min(len);
        (all[start..end].to_vec(), start)
    }

    fn build_timestamps(&self, start: usize, chart_w: usize) -> Vec<i64> {
        self.live_ticks.iter().skip(start).take(chart_w)
            .map(|s| s.timestamp)
            .collect()
    }

    fn window_10s_at<F>(&self, idx: usize, extract: &F) -> f32
    where
        F: Fn(&MetricSample) -> f32,
    {
        let window = SMOOTH_WINDOW;
        let start = idx.saturating_sub(window - 1);
        let count = (idx + 1 - start).min(window);
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

    fn render_disconnected(&self, mut terminal: DefaultTerminal, err: &str) -> anyhow::Result<()> {
        let msg = format!("Cannot connect to sb-daemon.\n\nError: {err}\n\nPress any key to exit.");
        terminal.draw(|f| {
            let block = Block::default()
                .title("sb — SteadyBearing")
                .borders(Borders::ALL);
            let text = Paragraph::new(msg).block(block).centered();
            f.render_widget(text, f.area());
        })?;
        crossterm::event::read()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_caret, follow_tail, historical_margin};

    #[test]
    fn caret_stays_in_the_central_80_percent() {
        let chart_w = 100;
        let margin = historical_margin(chart_w);
        assert_eq!(margin, 10);
        assert_eq!(clamp_caret(0, chart_w), margin);
        assert_eq!(clamp_caret(usize::MAX, chart_w), chart_w - margin - 1);
        assert_eq!(clamp_caret(50, chart_w), 50);
    }

    #[test]
    fn caret_clamp_never_panics_on_narrow_charts() {
        // Lower bound would exceed the upper bound if we used `clamp`.
        for chart_w in 0..=4 {
            let result = clamp_caret(usize::MAX, chart_w);
            assert!(result <= chart_w);
        }
    }

    #[test]
    fn follow_tail_keeps_the_user_at_the_end() {
        assert_eq!(follow_tail(100, 99, 110), 109);
    }

    #[test]
    fn follow_tail_preserves_distance_from_the_end() {
        assert_eq!(follow_tail(100, 90, 110), 100);
    }

    #[test]
    fn follow_tail_clamps_when_data_shrinks() {
        assert_eq!(follow_tail(100, 0, 10), 0);
    }

    #[test]
    fn follow_tail_handles_empty_data() {
        assert_eq!(follow_tail(100, 50, 0), 0);
        assert_eq!(follow_tail(0, 0, 5), 4);
    }
}
