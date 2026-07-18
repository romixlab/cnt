use cnt_core::{Counters, CountersBlock};
use color_eyre::Result;
use crossterm::event::{self, KeyCode};
use probe_rs::probe::list::Lister;
use probe_rs::{MemoryInterface, Permissions};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table, TableState};
use std::time::Duration;

pub fn tui(counters: &mut Counters) -> Result<()> {
    color_eyre::install()?;
    let counters = counters.ram_counters_mut().unwrap();

    let lister = Lister::new();
    let probes = lister.list_all();
    let probe = probes[0].open()?;
    let mut session = probe.attach("STM32H533RE", Permissions::default())?;
    let mut core = session.core(0)?;

    let mut table_state = TableState::default();
    table_state.select_first();
    table_state.select_first_column();
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| render(frame, &mut table_state, counters))?;
            if event::poll(Duration::from_millis(16))? {
                let event = event::read()?;
                if let Some(key) = event.as_key_press_event() {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('j') | KeyCode::Down => table_state.select_next(),
                        KeyCode::Char('k') | KeyCode::Up => table_state.select_previous(),
                        KeyCode::Char('l') | KeyCode::Right => table_state.select_next_column(),
                        KeyCode::Char('h') | KeyCode::Left => table_state.select_previous_column(),
                        KeyCode::Char('g') => table_state.select_first(),
                        KeyCode::Char('G') => table_state.select_last(),
                        _ => {}
                    }
                }
            }

            let mut data = vec![0u8; counters.buffer().size as usize];
            core.read_mem_32bit(counters.buffer().addr, &mut data)?;
            counters.read_values(&data).unwrap();
        }
    })
}

fn render(frame: &mut Frame, table_state: &mut TableState, counters: &CountersBlock) {
    let layout = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).spacing(1);
    let [top, main] = frame.area().layout(&layout);

    let title = Line::from_iter([
        Span::from("Counters").bold(),
        Span::from(" (Press 'q' to quit and arrow keys to navigate)"),
    ]);
    frame.render_widget(title.centered(), top);

    render_table(frame, main, table_state, counters);
}

pub fn render_table(
    frame: &mut Frame,
    area: Rect,
    table_state: &mut TableState,
    counters: &CountersBlock,
) {
    let header = Row::new(["Name", "Value", "Ty", "Severity", "Location"])
        .style(Style::new().bold())
        .bottom_margin(1);

    // let footer = Row::new([]);
    let widths = [
        Constraint::Percentage(30),
        Constraint::Percentage(10),
        Constraint::Percentage(10),
        Constraint::Percentage(10),
        Constraint::Percentage(50),
    ];
    let rows = counters.values_opt().map(|(cnt, value)| {
        Row::new([
            Cell::new(cnt.name.as_str()),
            Cell::new(format!("{:?}", value)),
            Cell::new(format!("{} {}", cnt.ty, cnt.storage)),
            Cell::new(format!("{:?}", cnt.severity)),
            Cell::new(
                cnt.location
                    .as_ref()
                    .map(|l| format!("{}:{}", l.file.display(), l.line))
                    .unwrap_or_default(),
            ),
        ])
    });

    let table = Table::new(rows, widths)
        .header(header)
        // .footer(footer.italic())
        .column_spacing(1)
        .style(Color::White)
        .row_highlight_style(Style::new().on_black().bold())
        .column_highlight_style(Color::Gray)
        .cell_highlight_style(Style::new().reversed().yellow())
        .highlight_symbol("🍴 ");

    frame.render_stateful_widget(table, area, table_state);
}
