//! Colour theme shared by the CLI (clap help and error output, subcommand output) and the TUI.
//!
//! All styles are derived from a single [`Palette`] of base colours. A 24-bit palette based on the IntelliJ "Dark"
//! scheme is used on terminals with truecolor support, with a 16-colour fallback for everything else.
//!
//! Styles are [`anstyle`] styles; the TUI converts them into `ratatui` styles with `.into()`.

use clap::builder::styling::{AnsiColor, Color, Effects, RgbColor, Style, Styles};
use cnt_core::Severity;
use std::sync::LazyLock;

/// Base colours, from which every style in [`Theme`] is derived.
struct Palette {
    /// Default text
    text: Color,
    /// Secondary text
    gray: Color,
    /// Background of the selected line
    selection: Color,
    red: Color,
    orange: Color,
    yellow: Color,
    olive: Color,
    green: Color,
    cyan: Color,
    blue: Color,
    purple: Color,
}

/// An RGB [`Color`].
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(RgbColor(r, g, b))
}

/// An ANSI [`Color`].
const fn ansi(color: AnsiColor) -> Color {
    Color::Ansi(color)
}

/// 24-bit palette, following the IntelliJ "Dark" editor colour scheme.
const DEFAULT_PALETTE: Palette = Palette {
    text: rgb(0xBC, 0xBE, 0xC4),      // default text
    gray: rgb(0x7A, 0x7E, 0x85),      // comment gray
    selection: rgb(0x21, 0x42, 0x83), // selection blue
    red: rgb(0xF7, 0x54, 0x64),       // error red
    orange: rgb(0xCF, 0x8E, 0x6D),    // keyword orange
    yellow: rgb(0xE0, 0xBB, 0x65),    // warning yellow
    olive: rgb(0xB3, 0xAE, 0x60),     // annotation olive
    green: rgb(0x6A, 0xAB, 0x73),     // string green
    cyan: rgb(0x2A, 0xAC, 0xB8),      // number cyan
    blue: rgb(0x56, 0xA8, 0xF5),      // function blue
    purple: rgb(0xC7, 0x7D, 0xBA),    // field purple
};

/// 16-colour approximation of [`DEFAULT_PALETTE`].
const FALLBACK_PALETTE: Palette = Palette {
    text: ansi(AnsiColor::BrightWhite),
    gray: ansi(AnsiColor::BrightBlack),
    selection: ansi(AnsiColor::Black),
    red: ansi(AnsiColor::Red),
    orange: ansi(AnsiColor::Yellow),
    yellow: ansi(AnsiColor::Yellow),
    olive: ansi(AnsiColor::BrightYellow),
    green: ansi(AnsiColor::Green),
    cyan: ansi(AnsiColor::Cyan),
    blue: ansi(AnsiColor::BrightBlue),
    purple: ansi(AnsiColor::Magenta),
};

/// Styles for the CLI and the TUI.
pub struct Theme {
    /// clap help and error output
    pub clap: Styles,
    /// Secondary information: hints, addresses, sizes
    pub hint: Style,
    /// File paths
    pub path: Style,
    /// Section and table headers
    pub header: Style,
    pub warn: Style,
    pub error: Style,
    severity: [Style; 5],
    /// TUI table body
    pub table: Style,
    /// TUI selected row
    pub row_highlight: Style,
    /// TUI selected column
    pub column_highlight: Style,
    /// TUI selected cell
    pub cell_highlight: Style,
}

impl Theme {
    const fn new(p: &Palette) -> Self {
        const BOLD_UNDER: Effects = Effects::BOLD.insert(Effects::UNDERLINE);
        Theme {
            clap: Styles::styled()
                .header(fg(p.orange).effects(BOLD_UNDER))
                .usage(fg(p.olive).bold())
                .literal(fg(p.blue).bold())
                .placeholder(fg(p.purple))
                .valid(fg(p.green).bold())
                .invalid(fg(p.red).bold())
                .error(fg(p.red).effects(BOLD_UNDER)),
            hint: fg(p.gray),
            path: fg(p.cyan),
            header: Style::new().bold(),
            warn: fg(p.yellow).bold(),
            error: fg(p.red).bold(),
            severity: [
                fg(p.red).bold(),
                fg(p.yellow),
                fg(p.green),
                fg(p.blue),
                fg(p.purple),
            ],
            table: fg(p.text),
            row_highlight: Style::new().bg_color(Some(p.selection)).bold(),
            column_highlight: fg(p.gray),
            cell_highlight: fg(p.yellow).invert(),
        }
    }

    /// Style for a non-zero counter of the given severity.
    pub fn severity(&self, severity: Severity) -> Style {
        self.severity[severity as usize]
    }
}

/// A plain style in `color`.
const fn fg(color: Color) -> Style {
    Style::new().fg_color(Some(color))
}

/// 24-bit theme, see [`DEFAULT_PALETTE`].
const DEFAULT_THEME: Theme = Theme::new(&DEFAULT_PALETTE);

/// 16-colour theme, see [`FALLBACK_PALETTE`].
const FALLBACK_THEME: Theme = Theme::new(&FALLBACK_PALETTE);

/// The theme for the current terminal. Colours are stripped by `anstream` when not writing to a terminal.
pub fn theme() -> &'static Theme {
    static THEME: LazyLock<&Theme> = LazyLock::new(|| {
        if truecolor() {
            &DEFAULT_THEME
        } else {
            &FALLBACK_THEME
        }
    });
    *THEME
}

/// Best-effort truecolor detection (`COLORTERM`, then known-good `TERM` values).
fn truecolor() -> bool {
    if let Ok(colorterm) = std::env::var("COLORTERM")
        && (colorterm.contains("truecolor") || colorterm.contains("24bit"))
    {
        return true;
    }
    std::env::var("TERM").is_ok_and(|term| {
        term.contains("truecolor") || term.contains("direct") || term.contains("24bit")
    })
}
