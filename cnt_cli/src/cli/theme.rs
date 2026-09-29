//! Colour theme for clap help and error output, and for the output of subcommands.
//!
//! A 24-bit colour palette based on the IntelliJ "Dark" scheme is used on terminals with truecolor support, with a
//! 16-colour fallback for everything else.

use clap::builder::styling::{AnsiColor, Color, Effects, RgbColor, Style, Styles};
use cnt_core::Severity;
use std::sync::LazyLock;

/// A palette of colours for the elements clap can style.
struct Palette {
    header: Color,
    usage: Color,
    literal: Color,
    placeholder: Color,
    valid: Color,
    invalid: Color,
    error: Color,
}

impl From<&Palette> for Styles {
    fn from(palette: &Palette) -> Self {
        /// A bold style in `color`.
        const fn bold(color: Color) -> Style {
            Style::new().fg_color(Some(color)).effects(Effects::BOLD)
        }

        /// A bold, underlined style in `color`.
        const fn bold_under(color: Color) -> Style {
            const BOLD_UNDER: Effects = Effects::BOLD.insert(Effects::UNDERLINE);
            Style::new().fg_color(Some(color)).effects(BOLD_UNDER)
        }

        Styles::styled()
            .header(bold_under(palette.header))
            .usage(bold(palette.usage))
            .literal(bold(palette.literal))
            .placeholder(Style::new().fg_color(Some(palette.placeholder)))
            .valid(bold(palette.valid))
            .invalid(bold(palette.invalid))
            .error(bold_under(palette.error))
    }
}

/// An RGB [`Color`].
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(RgbColor(r, g, b))
}

/// 24-bit palette, following the IntelliJ "Dark" editor colour scheme.
const DEFAULT_PALETTE: Palette = Palette {
    header: rgb(0xCF, 0x8E, 0x6D),      // keyword orange
    usage: rgb(0xB3, 0xAE, 0x60),       // annotation olive
    literal: rgb(0x56, 0xA8, 0xF5),     // function blue
    placeholder: rgb(0xC7, 0x7D, 0xBA), // field purple
    valid: rgb(0x6A, 0xAB, 0x73),       // string green
    invalid: rgb(0xFA, 0x66, 0x75),     // warning red
    error: rgb(0xF7, 0x54, 0x64),       // error red
};

/// 16-colour approximation of [`DEFAULT_PALETTE`].
const FALLBACK_PALETTE: Palette = Palette {
    header: Color::Ansi(AnsiColor::Yellow),
    usage: Color::Ansi(AnsiColor::BrightYellow),
    literal: Color::Ansi(AnsiColor::BrightBlue),
    placeholder: Color::Ansi(AnsiColor::Magenta),
    valid: Color::Ansi(AnsiColor::Green),
    invalid: Color::Ansi(AnsiColor::BrightRed),
    error: Color::Ansi(AnsiColor::Red),
};

/// Styles for the output of subcommands.
pub struct Theme {
    /// Secondary information: hints, addresses, sizes
    pub hint: Style,
    /// File paths
    pub path: Style,
    /// Section headers
    pub header: Style,
    pub warn: Style,
    pub error: Style,
    severity: [Style; 5],
}

impl Theme {
    /// Style for a non-zero counter of the given severity.
    pub fn severity(&self, severity: Severity) -> Style {
        self.severity[severity as usize]
    }
}

/// 24-bit output theme, following the IntelliJ "Dark" editor colour scheme.
const DEFAULT_THEME: Theme = Theme {
    hint: Style::new().fg_color(Some(rgb(0x7A, 0x7E, 0x85))), // comment gray
    path: Style::new().fg_color(Some(rgb(0x2A, 0xAC, 0xB8))), // number cyan
    header: Style::new().effects(Effects::BOLD),
    warn: Style::new()
        .fg_color(Some(rgb(0xE0, 0xBB, 0x65)))
        .effects(Effects::BOLD),
    error: Style::new()
        .fg_color(Some(rgb(0xF7, 0x54, 0x64)))
        .effects(Effects::BOLD),
    severity: [
        Style::new()
            .fg_color(Some(rgb(0xF7, 0x54, 0x64)))
            .effects(Effects::BOLD), // error
        Style::new().fg_color(Some(rgb(0xE0, 0xBB, 0x65))), // warn
        Style::new().fg_color(Some(rgb(0x6A, 0xAB, 0x73))), // info
        Style::new().fg_color(Some(rgb(0x56, 0xA8, 0xF5))), // debug
        Style::new().fg_color(Some(rgb(0xC7, 0x7D, 0xBA))), // trace
    ],
};

/// 16-colour approximation of [`DEFAULT_THEME`].
const FALLBACK_THEME: Theme = Theme {
    hint: Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
    path: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
    header: Style::new().effects(Effects::BOLD),
    warn: Style::new()
        .fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
        .effects(Effects::BOLD),
    error: Style::new()
        .fg_color(Some(Color::Ansi(AnsiColor::Red)))
        .effects(Effects::BOLD),
    severity: [
        Style::new()
            .fg_color(Some(Color::Ansi(AnsiColor::Red)))
            .effects(Effects::BOLD),
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))),
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Magenta))),
    ],
};

/// The output theme for the current terminal. Colours are stripped by `anstream` when not writing to a terminal.
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

/// Pick the richest palette the current terminal can render.
pub fn select_style() -> Styles {
    if truecolor() {
        (&DEFAULT_PALETTE).into()
    } else {
        (&FALLBACK_PALETTE).into()
    }
}
