use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use sb_protocol::types::{HistoricalRow, MetricSample, FAN_MAX_RPM, FAN_MIN_RPM};

use crate::chart;
use crate::historical::{self, DisplayMode, HistState, MetricKind};

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

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

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

        self.live_ticks = std::mem::take(&mut snapshot.tier1).into();
        crate::client::request_stream_live(&mut framed).await?;

        let tick_rate = Duration::from_millis(TICK_RATE_MS);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|f| self.render(f))?;

            let timeout = if self.timeframe == TimeFrame::Live {
                tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or(Duration::ZERO)
            } else {
                Duration::from_secs(HISTORICAL_POLL_SECS)
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
                            KeyCode::Char('t') => {
                                self.key_repeat = None;
                                let new = self.timeframe.next();
                                if let Err(e) = self.switch_timeframe(&mut framed, new).await {
                                    self.render_disconnected(terminal, &e.to_string())?;
                                    return Ok(());
                                }
                                self.selection = self.historical_data.len().saturating_sub(1);
                                let margin = (self.chart_w / 10).max(1);
                                self.caret_col = self.chart_w.saturating_sub(margin + 1);
                            }
                            KeyCode::Left => {
                                if self.timeframe != TimeFrame::Live {
                                    let step = self.advance_repeat(KeyCode::Left);
                                    self.selection = self.selection.saturating_sub(step);
                                    let margin = (self.chart_w / 10).max(1);
                                    self.caret_col = self.caret_col
                                        .saturating_sub(step)
                                        .clamp(margin, self.chart_w.saturating_sub(margin + 1));
                                }
                            }
                            KeyCode::Right => {
                                if self.timeframe != TimeFrame::Live {
                                    let step = self.advance_repeat(KeyCode::Right);
                                    let data_len = self.historical_data.len();
                                    self.selection =
                                        (self.selection + step).min(data_len.saturating_sub(1));
                                    let margin = (self.chart_w / 10).max(1);
                                    self.caret_col = (self.caret_col + step)
                                        .clamp(margin, self.chart_w.saturating_sub(margin + 1));
                                }
                            }
                            _ => {
                                self.key_repeat = None;
                            }
                        }
                    }
                }
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
            let mut snap = crate::client::request_snapshot(framed).await?;
            self.live_ticks = std::mem::take(&mut snap.tier1).into();
            self.timeframe = TimeFrame::Live;
            self.historical_data.clear();
            crate::client::request_stream_live(framed).await?;
            return Ok(());
        }

        let bucket_secs = match new {
            TimeFrame::Minute => 60_u32,
            TimeFrame::Hourly => 3600,
            TimeFrame::Live => unreachable!(),
        };

        let mut conn = crate::client::connect(&self.socket_path).await?;
        self.historical_data =
            crate::client::request_historical(&mut conn, bucket_secs, 2000).await?;
        self.timeframe = new;
        self.selection = self.historical_data.len().saturating_sub(1);
        self.caret_col = usize::MAX; // clamp in next render
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
        if chart_w != self.chart_w {
            self.chart_w = chart_w;
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

        self.render_metric_pane(
            f,
            chunks[0],
            "CPU Usage",
            latest,
            hist.as_ref(),
            sel_usage,
            MetricKind::Usage,
        );
        self.render_metric_pane(
            f,
            chunks[1],
            "CPU Temp",
            latest,
            hist.as_ref(),
            sel_temp,
            MetricKind::Temp,
        );
        self.render_metric_pane(
            f,
            chunks[2],
            "Fan",
            latest,
            hist.as_ref(),
            sel_fan,
            MetricKind::Fan,
        );

        let scroll_status = time_str.unwrap_or_default();
        let mode_label = if self.timeframe == TimeFrame::Live {
            self.mode.label()
        } else {
            self.mode.label_historical()
        };
        let line = Line::from(format!(
            " m: {}  t: {}{}  q: quit",
            mode_label,
            self.timeframe.label(),
            scroll_status,
        ));
        f.render_widget(line, chunks[3]);
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
            );
            return;
        }

        let Some(s) = latest else { return };

        let inner = Block::default().borders(Borders::ALL).inner(area);
        let chart_w = inner.width.max(1) as usize;
        let chart_h = inner.height.max(1) as usize;

        let (cur_val, values, range_min, range_max) = self.extract_bars(chart_w, &s, kind);

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

        let mut lines = Vec::new();
        for visual_row in (0..chart_h).rev() {
            let mut spans = Vec::new();
            for &val in &buckets {
                let norm = ((val - range_min) / (range_max - range_min)).clamp(0.0, 1.0);
                let bar_top = norm * chart_h as f32;
                let fill = (bar_top - visual_row as f32).clamp(0.0, 1.0);
                let ch = BLOCKS[(fill * 8.0).round() as usize];
                let color = match kind {
                    MetricKind::Usage => historical::usage_color(val),
                    MetricKind::Temp => historical::temp_color(val),
                    MetricKind::Fan => historical::fan_color(val as u16),
                };
                spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    // ── live data helpers ──────────────────────────────────────────────

    fn extract_bars(
        &self,
        chart_w: usize,
        latest: &MetricSample,
        kind: MetricKind,
    ) -> (f32, Vec<f32>, f32, f32) {
        match kind {
            MetricKind::Usage => {
                let cur = self.current_val(latest, |t| t.cpu_usage);
                let vals = self.build_values(chart_w, |t| t.cpu_usage);
                (cur, vals, 0.0, 100.0)
            }
            MetricKind::Temp => {
                let cur = self.current_val(latest, |t| t.cpu_temp);
                let vals = self.build_values(chart_w, |t| t.cpu_temp);
                (cur, vals, 0.0, 100.0)
            }
            MetricKind::Fan => {
                let cur = latest.fan_rpm;
                let vals = self.build_values(chart_w, |t| t.fan_rpm as f32);
                let floor = FAN_MIN_RPM as f32 * FAN_FLOOR_MULT;
                (cur as f32, vals, floor, FAN_MAX_RPM as f32)
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

    fn build_values<F>(&self, chart_w: usize, extract: F) -> Vec<f32>
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
        all[start..end].to_vec()
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
        ratatui::restore();
        Ok(())
    }
}
