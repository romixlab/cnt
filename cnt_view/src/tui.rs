//! The terminal UI: a table per buffer, values highlighted as they change, the firmware's logs below.

use crate::list::severity_label;
use crate::read::level_label;
use crate::theme::theme;
use anstream::println;
use cnt_core::{Counter, Counters, CountersBlock, Event, Log, Severity, Source, Storage, Value};
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use human_repr::HumanCount;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Cell, HighlightSpacing, Paragraph, Row, Table, TableState,
};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How often counters are read.
const REFRESH_INTERVAL: Duration = Duration::from_millis(50);
/// How often defmt logs are read. RTT buffers are small, and the firmware blocks when they are full.
const LOG_INTERVAL: Duration = Duration::from_millis(10);
/// Number of log messages kept.
const LOG_HISTORY: usize = 1000;
/// How long a value stays highlighted after it changed.
const CHANGE_HIGHLIGHT: Duration = Duration::from_secs(1);
/// Rows taken by a panel besides the counters: borders, table header and its margin.
const PANEL_OVERHEAD: u16 = 4;
const HIGHLIGHT_SYMBOL: &str = "▶ ";
const HIGHLIGHT_SYMBOL_WIDTH: u16 = 2;
const COLUMN_SPACING: u16 = 2;

/// Read the counters (and logs) once and draw one frame of the TUI on `terminal`: to embed the view, or to check
/// what it shows with ratatui's `TestBackend`.
pub fn draw_once<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    counters: &mut Counters,
    source: &mut dyn Source,
    title: &str,
) -> anyhow::Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut app = App::new(counters, source.logs_attached().is_some(), title);
    app.refresh(source);
    app.poll_logs(source);
    if let Some(e) = &app.read_error {
        anyhow::bail!("{e}");
    }
    terminal.draw(|frame| app.render(frame))?;
    Ok(())
}

/// Run the TUI until the user quits. `title` names where the counters come from (shown in the title line), the
/// source is closed at the end, also after an error.
pub fn tui(counters: &mut Counters, source: &mut dyn Source, title: &str) -> anyhow::Result<()> {
    if counters.is_empty() {
        let hint = theme().hint;
        println!("{hint}No counters found{hint:#}");
        return Ok(());
    }

    let mut app = App::new(counters, source.logs_attached().is_some(), title);
    app.refresh(source);
    let result: std::io::Result<()> = ratatui::run(|terminal| {
        let mut last_refresh = Instant::now();
        let mut redraw = true;
        loop {
            if redraw {
                terminal.draw(|frame| app.render(frame))?;
                redraw = false;
            }
            let mut timeout = REFRESH_INTERVAL.saturating_sub(last_refresh.elapsed());
            if app.has_logs {
                timeout = timeout.min(LOG_INTERVAL);
            }
            if event::poll(timeout)? {
                if let Some(key) = event::read()?.as_key_press_event()
                    && !app.handle_key(key, source)
                {
                    return Ok(());
                }
                // Key presses and resizes
                redraw = true;
            }
            redraw |= app.poll_logs(source);
            if last_refresh.elapsed() >= REFRESH_INTERVAL {
                app.refresh(source);
                last_refresh = Instant::now();
                redraw = true;
            }
        }
    });
    drop(app);
    // Also after errors, as the firmware blocks on a full log buffer otherwise
    let closed = source.close();
    result?;
    closed
}

/// A table of RAM or BKP counters.
struct Panel {
    storage: Storage,
    state: TableState,
}

struct App<'a> {
    counters: &'a mut Counters,
    panels: Vec<Panel>,
    title: String,
    /// Index of the panel receiving navigation keys
    focus: usize,
    /// When each counter last changed, keyed by its storage and index
    changed: HashMap<(Storage, u64), Instant>,
    /// Outcome of the last user action
    status: Option<Status>,
    /// Last failed read, cleared on the next successful one
    read_error: Option<String>,
    /// The source carries logs: they get a panel
    has_logs: bool,
    /// Logs are received, not waiting for the firmware to set them up
    logs_attached: bool,
    logs: VecDeque<Log>,
    /// Last failed log read, cleared on the next successful one
    log_error: Option<String>,
}

enum Status {
    Info(String),
    Error(String),
}

impl<'a> App<'a> {
    fn new(counters: &'a mut Counters, has_logs: bool, title: &str) -> Self {
        let panels = counters
            .blocks()
            .map(|b| Panel {
                storage: b.storage(),
                state: TableState::default().with_selected(0),
            })
            .collect();
        App {
            counters,
            panels,
            title: title.to_string(),
            focus: 0,
            changed: HashMap::new(),
            status: None,
            read_error: None,
            has_logs,
            logs_attached: false,
            logs: VecDeque::new(),
            log_error: None,
        }
    }

    /// Returns false when the TUI should exit.
    fn handle_key(&mut self, key: KeyEvent, source: &mut dyn Source) -> bool {
        let state = &mut self.panels[self.focus].state;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            // Raw mode turns Ctrl+C into a key press
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
            KeyCode::Char('c') => self.logs.clear(),
            KeyCode::Tab | KeyCode::BackTab => self.focus = (self.focus + 1) % self.panels.len(),
            KeyCode::Char('j') | KeyCode::Down => state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => state.select_previous(),
            KeyCode::PageDown => state.scroll_down_by(10),
            KeyCode::PageUp => state.scroll_up_by(10),
            KeyCode::Char('g') | KeyCode::Home => state.select_first(),
            KeyCode::Char('G') | KeyCode::End => state.select_last(),
            KeyCode::Char('r') => self.reset(Storage::Ram, source),
            // Separate key, as BKP counters are meant to survive resets and power loss
            KeyCode::Char('R') => self.reset(Storage::Bkp, source),
            _ => {}
        }
        true
    }

    fn reset(&mut self, storage: Storage, source: &mut dyn Source) {
        if !self.panels.iter().any(|p| p.storage == storage) {
            self.status = Some(Status::Error(format!("No {storage} counters")));
            return;
        }
        self.status = Some(match self.counters.reset(storage, source) {
            Ok(()) => Status::Info(format!("{storage} counters reset")),
            Err(e) => Status::Error(format!("Failed to reset {storage} counters: {e:#}")),
        });
        self.refresh(source);
    }

    /// Read all counters, noting which ones changed.
    fn refresh(&mut self, source: &mut dyn Source) {
        let now = Instant::now();
        match self.counters.read_changes(source) {
            Ok(changes) => {
                for c in changes {
                    // Resets are not interesting, only counters firing
                    if c.event != Event::Initial && c.value.to_u64() != 0 {
                        self.changed.insert((c.storage, c.counter.idx), now);
                    }
                }
                self.read_error = None;
            }
            Err(e) => self.read_error = Some(format!("Failed to read counters: {e:#}")),
        }
    }

    /// Read new logs, returns whether there is anything new to show.
    fn poll_logs(&mut self, source: &mut dyn Source) -> bool {
        if !self.has_logs {
            return false;
        }
        let attached = source.logs_attached() == Some(true);
        let changed = attached != self.logs_attached;
        self.logs_attached = attached;
        let new = match source.poll_logs() {
            Ok(logs) => {
                let had_error = self.log_error.take().is_some();
                let new = !logs.is_empty();
                self.logs.extend(logs);
                let excess = self.logs.len().saturating_sub(LOG_HISTORY);
                self.logs.drain(..excess);
                had_error || new
            }
            Err(e) => {
                self.log_error = Some(format!("Failed to read logs: {e:#}"));
                true
            }
        };
        changed || new
    }

    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]);
        let [title, main, status, keys] = frame.area().layout(&layout);

        frame.render_widget(self.title().centered(), title);
        let counters = if self.has_logs {
            // Counters take what they need, but leave at least a third to the logs
            let height = self.panels_height().min(main.height * 2 / 3);
            let [counters, logs] = main.layout(&Layout::vertical([
                Constraint::Length(height),
                Constraint::Fill(1),
            ]));
            self.render_logs(frame, logs);
            counters
        } else {
            main
        };
        self.render_panels(frame, counters);
        frame.render_widget(self.status_line(), status);
        frame.render_widget(keys_line(self.panels.len() > 1, self.has_logs), keys);
    }

    /// `Counters · 2 of 14 non-zero`
    fn title(&self) -> Line<'static> {
        let t = theme();
        let (total, fired) = self
            .counters
            .blocks()
            .flat_map(|b| b.values())
            .fold((0, 0), |(total, fired), (_, v)| {
                (total + 1, fired + (v.to_u64() != 0) as usize)
            });
        let fired_style = if fired == 0 {
            Style::from(t.hint)
        } else {
            Style::from(t.warn)
        };
        let source = if self.title.is_empty() {
            String::new()
        } else {
            format!(" · {}", self.title)
        };
        Line::from_iter([
            Span::styled("Counters", Style::from(t.header)),
            Span::styled(format!("{source} · "), Style::from(t.hint)),
            Span::styled(fired.to_string(), fired_style),
            Span::styled(format!(" of {total} non-zero"), Style::from(t.hint)),
        ])
    }

    /// Rows needed to show all panels in full.
    fn panels_height(&self) -> u16 {
        self.counters
            .blocks()
            .map(|b| b.entries().len() as u16 + PANEL_OVERHEAD)
            .sum()
    }

    /// The most recent logs that fit, newest at the bottom.
    fn render_logs(&self, frame: &mut Frame, area: Rect) {
        let t = theme();
        let hint = Style::from(t.hint);
        let title = Line::from_iter([
            Span::raw(" 📜 "),
            Span::styled("defmt logs", Style::from(t.header)),
            Span::styled(
                if self.logs_attached {
                    " "
                } else {
                    " waiting for the firmware "
                },
                hint,
            ),
        ]);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(hint)
            .title(title);
        let height = block.inner(area).height as usize;

        let mut lines = vec![];
        for log in self.logs.iter().rev() {
            if lines.len() >= height {
                break;
            }
            lines.push(log_lines(log));
        }
        let mut lines: Vec<Line> = lines.into_iter().rev().flatten().collect();
        let excess = lines.len().saturating_sub(height);
        lines.drain(..excess);

        let logs = Paragraph::new(lines)
            .block(block)
            .style(Style::from(t.table));
        frame.render_widget(logs, area);
    }

    fn render_panels(&mut self, frame: &mut Frame, area: Rect) {
        // Give each panel as many rows as it needs, sharing the space proportionally if there is not enough
        let heights: Vec<u16> = self
            .counters
            .blocks()
            .map(|b| b.entries().len() as u16 + PANEL_OVERHEAD)
            .collect();
        let constraints: Vec<Constraint> = if heights.iter().sum::<u16>() <= area.height {
            heights.into_iter().map(Constraint::Length).collect()
        } else {
            heights.into_iter().map(Constraint::Fill).collect()
        };
        let areas = Layout::vertical(constraints).split(area);

        // Align columns across panels
        let name_width = self
            .counters
            .blocks()
            .flat_map(|b| b.entries().values())
            .map(|c| c.qualified_name().len())
            .max()
            .unwrap_or(0) as u16;
        let now = Instant::now();
        let panels = self.panels.iter_mut().zip(self.counters.blocks());
        for (i, ((panel, block), area)) in panels.zip(areas.iter()).enumerate() {
            let view = PanelView {
                focused: i == self.focus,
                name_width,
                changed: &self.changed,
                now,
            };
            view.render(frame, *area, block, &mut panel.state);
        }
    }

    fn status_line(&self) -> Line<'static> {
        let t = theme();
        let error = self.read_error.as_ref().or(self.log_error.as_ref());
        let (text, style) = match (error, &self.status) {
            (Some(e), _) | (None, Some(Status::Error(e))) => (format!("🛑 {e}"), t.error),
            (None, Some(Status::Info(s))) => (format!("✔ {s}"), t.hint),
            (None, None) => return Line::default(),
        };
        Line::styled(text, Style::from(style))
    }
}

/// Everything needed to render a panel, besides the panel itself.
struct PanelView<'a> {
    focused: bool,
    name_width: u16,
    changed: &'a HashMap<(Storage, u64), Instant>,
    now: Instant,
}

impl PanelView<'_> {
    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        counters: &CountersBlock,
        state: &mut TableState,
    ) {
        let t = theme();
        let hint = Style::from(t.hint);

        let buffer = counters.buffer();
        let used = counters.used_bytes();
        let title = Line::from_iter([
            Span::raw(" 📍 "),
            Span::styled(
                format!("{} counters", counters.storage()),
                Style::from(t.header),
            ),
            Span::styled(
                format!(
                    " at 0x{:08x}, {used} of {} B used ",
                    buffer.addr, buffer.size
                ),
                hint,
            ),
        ]);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                Style::from(t.table)
            } else {
                hint
            })
            .title(title);

        let header = Row::new(["Severity", "Name", "Value", "Type", "Location"])
            .style(Style::from(t.header))
            .bottom_margin(1);
        let fixed = [8, self.name_width.max(4), 12, 4];
        let widths = fixed
            .map(Constraint::Length)
            .into_iter()
            .chain([Constraint::Fill(1)]);
        // Borders, highlight symbol, fixed columns and spacing between all columns
        let location_width = area.width.saturating_sub(
            2 + HIGHLIGHT_SYMBOL_WIDTH
                + fixed.iter().sum::<u16>()
                + COLUMN_SPACING * fixed.len() as u16,
        ) as usize;
        let rows = counters
            .values_opt()
            .map(|(cnt, value)| self.row(counters.storage(), cnt, value, location_width));

        let mut table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .column_spacing(COLUMN_SPACING)
            .style(Style::from(t.table))
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol(HIGHLIGHT_SYMBOL);
        if self.focused {
            table = table.row_highlight_style(Style::from(t.row_highlight));
        } else {
            table = table.highlight_symbol(" ".repeat(HIGHLIGHT_SYMBOL_WIDTH as usize));
        }

        frame.render_stateful_widget(table, area, state);
    }

    fn row<'c>(
        &self,
        storage: Storage,
        cnt: &'c Counter,
        value: Option<Value>,
        location_width: usize,
    ) -> Row<'c> {
        let t = theme();
        let hint = Style::from(t.hint);
        // Only draw attention to counters that have fired
        let fired = value.is_some_and(|v| v.to_u64() != 0);
        let style = if fired {
            Style::from(t.severity(cnt.severity))
        } else {
            hint
        };
        let recently_changed = self
            .changed
            .get(&(storage, cnt.idx))
            .is_some_and(|at| self.now.duration_since(*at) < CHANGE_HIGHLIGHT);
        let value_style = if recently_changed {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        };

        Row::new([
            Cell::new(format!(
                "{} {}",
                severity_icon(cnt.severity),
                severity_label(cnt.severity)
            ))
            .style(style),
            Cell::new(cnt.qualified_name()),
            Cell::new(Line::styled(format_value(value, &cnt.unit), value_style).right_aligned()),
            Cell::new(cnt.ty.to_string()).style(hint),
            Cell::new(self.location(cnt, location_width)),
        ])
    }

    /// defmt-style location: `crate [Layout] @ file:line`.
    ///
    /// If it does not fit in `width`, the crate and then the file are shortened from the left, so that the most
    /// specific part and the line number stay visible.
    fn location(&self, cnt: &Counter, width: usize) -> Line<'static> {
        let t = theme();
        let hint = Style::from(t.hint);
        let file = truncate_left(&cnt.location.to_string(), width);
        let mut spans = vec![];
        let origin = match &cnt.layout {
            Some(layout) => format!("{} {layout}", cnt.crate_name),
            None => cnt.crate_name.to_string(),
        };
        let origin_width = width.saturating_sub(file.chars().count() + " @ ".len());
        // A few characters of a crate name are just noise
        if origin_width >= 8 {
            let origin = truncate_left(&origin, origin_width);
            spans.push(Span::styled(format!("{origin} @ "), hint));
        }
        spans.push(Span::styled(file, Style::from(t.path)));
        Line::from(spans)
    }
}

/// Keep the last `width` characters of `s`, marking the cut with `…`.
fn truncate_left(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let tail: String = s.chars().skip(len - width + 1).collect();
    format!("…{tail}")
}

/// Single-width symbol for a severity, so that it can be coloured (unlike emoji).
fn severity_icon(severity: Severity) -> char {
    match severity {
        Severity::Error => '✖',
        Severity::Warn => '▲',
        Severity::Info => '●',
        Severity::Debug => '◆',
        Severity::Trace => '◇',
    }
}

fn format_value(value: Option<Value>, unit: &str) -> String {
    match value {
        None => "n/a".to_string(),
        Some(v) if unit == "B" => v.to_u64().human_count_bytes().to_string(),
        Some(v) if unit.is_empty() => v.to_string(),
        Some(v) => format!("{v} {unit}"),
    }
}

/// `0.000123 INFO  message @ src/main.rs:42`, continuation lines of the message indented below it.
fn log_lines(log: &Log) -> Vec<Line<'static>> {
    let t = theme();
    let hint = Style::from(t.hint);
    let style = log
        .severity
        .map_or(Style::from(t.table), |s| Style::from(t.severity(s)));
    let mut prefix = vec![];
    if let Some(ts) = &log.timestamp {
        prefix.push(Span::styled(format!("{ts} "), hint));
    }
    prefix.push(Span::styled(format!("{} ", level_label(log)), style));
    let indent = " ".repeat(prefix.iter().map(|s| s.width()).sum());

    let mut lines: Vec<Line> = log
        .message
        .lines()
        .enumerate()
        .map(|(i, text)| {
            let mut spans = if i == 0 {
                prefix.clone()
            } else {
                vec![Span::raw(indent.clone())]
            };
            spans.push(Span::raw(text.to_string()));
            Line::from(spans)
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(prefix));
    }
    if let Some(file_line) = log.file_line() {
        lines[0].push_span(Span::styled(" @ ", hint));
        lines[0].push_span(Span::styled(file_line, Style::from(t.path)));
    }
    lines
}

/// Keyboard shortcuts help.
fn keys_line(multiple_panels: bool, logs: bool) -> Line<'static> {
    let t = theme();
    let (key, hint) = (Style::from(t.key), Style::from(t.hint));
    let mut keys = vec![("q", "quit"), ("↑↓/jk", "select"), ("g/G", "first/last")];
    if multiple_panels {
        keys.push(("Tab", "switch table"));
    }
    keys.extend([("r", "reset RAM"), ("R", "reset BKP")]);
    if logs {
        keys.push(("c", "clear logs"));
    }
    let spans = keys.into_iter().enumerate().flat_map(|(i, (k, desc))| {
        let sep = if i == 0 { "" } else { "  " };
        [
            Span::raw(sep),
            Span::styled(k, key),
            Span::styled(format!(" {desc}"), hint),
        ]
    });
    Line::from_iter(spans)
}
