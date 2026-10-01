use crate::cli::list::severity_label;
use crate::cli::logs::{DefmtReader, Log};
use crate::cli::reset::reset;
use crate::theme::theme;
use anstream::println;
use cnt_core::{Counter, Counters, CountersBlock, Severity, Storage, Value};
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use human_repr::HumanCount;
use probe_rs::{Core, MemoryInterface};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Cell, HighlightSpacing, Paragraph, Row, Table, TableState,
};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How often counters are read from the target.
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

pub fn tui(
    counters: &mut Counters,
    core: &mut Core,
    defmt: Option<DefmtReader>,
) -> anyhow::Result<()> {
    if counters.is_empty() {
        let hint = theme().hint;
        println!("{hint}No counters found{hint:#}");
        return Ok(());
    }

    let mut app = App::new(counters, defmt);
    app.refresh(core);
    let result: std::io::Result<()> = ratatui::run(|terminal| {
        let mut last_refresh = Instant::now();
        let mut redraw = true;
        loop {
            if redraw {
                terminal.draw(|frame| app.render(frame))?;
                redraw = false;
            }
            let mut timeout = REFRESH_INTERVAL.saturating_sub(last_refresh.elapsed());
            if app.defmt.is_some() {
                timeout = timeout.min(LOG_INTERVAL);
            }
            if event::poll(timeout)? {
                if let Some(key) = event::read()?.as_key_press_event()
                    && !app.handle_key(key, core)
                {
                    return Ok(());
                }
                // Key presses and resizes
                redraw = true;
            }
            redraw |= app.poll_logs(core);
            if last_refresh.elapsed() >= REFRESH_INTERVAL {
                app.refresh(core);
                last_refresh = Instant::now();
                redraw = true;
            }
        }
    });
    // Also after errors, as the firmware blocks on a full log buffer otherwise
    let detached = app.defmt.as_mut().map_or(Ok(()), |d| d.detach(core));
    result?;
    detached
}

/// A table of RAM or BKP counters.
struct Panel<'a> {
    counters: &'a mut CountersBlock,
    state: TableState,
}

struct App<'a> {
    panels: Vec<Panel<'a>>,
    /// Index of the panel receiving navigation keys
    focus: usize,
    /// When each counter last changed, keyed by its address in target memory
    changed: HashMap<u64, Instant>,
    /// Outcome of the last user action
    status: Option<Status>,
    /// Last failed read, cleared on the next successful one
    read_error: Option<String>,
    defmt: Option<DefmtReader>,
    logs: VecDeque<Log>,
    /// Last failed log read, cleared on the next successful one
    log_error: Option<String>,
}

enum Status {
    Info(String),
    Error(String),
}

impl<'a> App<'a> {
    fn new(counters: &'a mut Counters, defmt: Option<DefmtReader>) -> Self {
        let panels = counters
            .blocks_mut()
            .map(|counters| Panel {
                counters,
                state: TableState::default().with_selected(0),
            })
            .collect();
        App {
            panels,
            focus: 0,
            changed: HashMap::new(),
            status: None,
            read_error: None,
            defmt,
            logs: VecDeque::new(),
            log_error: None,
        }
    }

    /// Returns false when the TUI should exit.
    fn handle_key(&mut self, key: KeyEvent, core: &mut Core) -> bool {
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
            KeyCode::Char('r') => self.reset(Storage::Ram, core),
            // Separate key, as BKP counters are meant to survive resets and power loss
            KeyCode::Char('R') => self.reset(Storage::Bkp, core),
            _ => {}
        }
        true
    }

    fn reset(&mut self, storage: Storage, core: &mut Core) {
        let Some(panel) = self.panels.iter().find(|p| p.counters.storage() == storage) else {
            self.status = Some(Status::Error(format!("No {storage} counters")));
            return;
        };
        self.status = Some(match reset(panel.counters, core) {
            Ok(()) => Status::Info(format!("{storage} counters reset")),
            Err(e) => Status::Error(format!("Failed to reset {storage} counters: {e:#}")),
        });
        self.refresh(core);
    }

    /// Read all counters from the target.
    fn refresh(&mut self, core: &mut Core) {
        let now = Instant::now();
        for panel in &mut self.panels {
            if let Err(e) = read_block(panel.counters, core, &mut self.changed, now) {
                let storage = panel.counters.storage();
                self.read_error = Some(format!("Failed to read {storage} counters: {e:#}"));
                return;
            }
        }
        self.read_error = None;
    }

    /// Read new defmt logs, returns whether there is anything new to show.
    fn poll_logs(&mut self, core: &mut Core) -> bool {
        let Some(defmt) = &mut self.defmt else {
            return false;
        };
        match defmt.poll(core) {
            Ok(logs) => {
                let had_error = self.log_error.take().is_some();
                self.logs.extend(logs);
                let excess = self.logs.len().saturating_sub(LOG_HISTORY);
                self.logs.drain(..excess);
                had_error || !self.logs.is_empty()
            }
            Err(e) => {
                self.log_error = Some(format!("Failed to read defmt logs: {e:#}"));
                true
            }
        }
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
        let counters = if self.defmt.is_some() {
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
        frame.render_widget(keys_line(self.panels.len() > 1, self.defmt.is_some()), keys);
    }

    /// `Counters · 2 of 14 non-zero`
    fn title(&self) -> Line<'static> {
        let t = theme();
        let (total, fired) = self
            .panels
            .iter()
            .flat_map(|p| p.counters.values())
            .fold((0, 0), |(total, fired), (_, v)| {
                (total + 1, fired + (v.to_u64() != 0) as usize)
            });
        let fired_style = if fired == 0 {
            Style::from(t.hint)
        } else {
            Style::from(t.warn)
        };
        Line::from_iter([
            Span::styled("Counters", Style::from(t.header)),
            Span::styled(" · ", Style::from(t.hint)),
            Span::styled(fired.to_string(), fired_style),
            Span::styled(format!(" of {total} non-zero"), Style::from(t.hint)),
        ])
    }

    /// Rows needed to show all panels in full.
    fn panels_height(&self) -> u16 {
        self.panels
            .iter()
            .map(|p| p.counters.entries().len() as u16 + PANEL_OVERHEAD)
            .sum()
    }

    /// The most recent logs that fit, newest at the bottom.
    fn render_logs(&self, frame: &mut Frame, area: Rect) {
        let t = theme();
        let hint = Style::from(t.hint);
        let attached = self.defmt.as_ref().is_some_and(|d| d.is_attached());
        let title = Line::from_iter([
            Span::raw(" 📜 "),
            Span::styled("defmt logs", Style::from(t.header)),
            Span::styled(if attached { " " } else { " waiting for RTT " }, hint),
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
            .panels
            .iter()
            .map(|p| p.counters.entries().len() as u16 + PANEL_OVERHEAD)
            .collect();
        let constraints: Vec<Constraint> = if heights.iter().sum::<u16>() <= area.height {
            heights.into_iter().map(Constraint::Length).collect()
        } else {
            heights.into_iter().map(Constraint::Fill).collect()
        };
        let areas = Layout::vertical(constraints).split(area);

        // Align columns across panels
        let name_width = self
            .panels
            .iter()
            .flat_map(|p| p.counters.entries().values())
            .map(|c| c.qualified_name().len())
            .max()
            .unwrap_or(0) as u16;
        let now = Instant::now();
        for (i, (panel, area)) in self.panels.iter_mut().zip(areas.iter()).enumerate() {
            let view = PanelView {
                focused: i == self.focus,
                name_width,
                changed: &self.changed,
                now,
            };
            view.render(frame, *area, panel);
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
    changed: &'a HashMap<u64, Instant>,
    now: Instant,
}

impl PanelView<'_> {
    fn render(&self, frame: &mut Frame, area: Rect, panel: &mut Panel) {
        let t = theme();
        let hint = Style::from(t.hint);
        let counters = &*panel.counters;

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
            .map(|(cnt, value)| self.row(cnt, value, location_width));

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

        frame.render_stateful_widget(table, area, &mut panel.state);
    }

    fn row<'c>(&self, cnt: &'c Counter, value: Option<Value>, location_width: usize) -> Row<'c> {
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
            .get(&cnt.addr)
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

/// Read a block of counters from the target, noting which ones changed.
fn read_block(
    counters: &mut CountersBlock,
    core: &mut Core,
    changed: &mut HashMap<u64, Instant>,
    now: Instant,
) -> anyhow::Result<()> {
    let previous: HashMap<u64, u64> = counters
        .values()
        .map(|(c, v)| (c.addr, v.to_u64()))
        .collect();

    let mut data = vec![0u8; counters.buffer().size as usize];
    core.read_mem_32bit(counters.buffer().addr, &mut data)?;
    counters.read_values(&data)?;

    for (c, v) in counters.values() {
        let v = v.to_u64();
        // Resets are not interesting, only counters firing
        if v != 0 && previous.get(&c.addr).is_some_and(|p| *p != v) {
            changed.insert(c.addr, now);
        }
    }
    Ok(())
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
        .severity()
        .map_or(Style::from(t.table), |s| Style::from(t.severity(s)));
    let mut prefix = vec![];
    if let Some(ts) = &log.timestamp {
        prefix.push(Span::styled(format!("{ts} "), hint));
    }
    prefix.push(Span::styled(format!("{} ", log.level_label()), style));
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
