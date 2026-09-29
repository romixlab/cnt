//! Colour theme for clap help and error output.
//!
//! A 24-bit colour palette based on the IntelliJ "Dark" scheme is used on terminals with truecolor support, with a
//! 16-colour fallback for everything else.

use clap::builder::styling::{AnsiColor, Color, Effects, RgbColor, Style, Styles};

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
