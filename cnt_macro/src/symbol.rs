use std::env;
// Borrowed from defmt
use std::fmt::Write;

pub(crate) fn mangled(tag: &str, data: &str) -> String {
    Symbol::new(tag, data).mangle()
}

struct Symbol<'a> {
    /// Name of the Cargo package in which the symbol is being instantiated. Used for avoiding
    /// symbol name collisions.
    package: String,

    /// Unique identifier that disambiguates otherwise equivalent invocations in the same crate.
    disambiguator: u64,

    /// Symbol categorization. Known values:
    /// * `cnt_` for
    /// * Anything starting with `defmt_` is reserved for use by defmt, other prefixes are free for
    ///   use by third-party apps (but they all should use a prefix!).
    tag: String,

    /// Symbol data for use by the host tooling. Interpretation depends on `tag`.
    data: &'a str,

    /// Crate name obtained via CARGO_CRATE_NAME (added since a Cargo package can contain many crates).
    crate_name: String,
}

impl<'a> Symbol<'a> {
    fn new(tag: &'a str, data: &'a str) -> Self {
        Self {
            // `CARGO_PKG_NAME` is set to the invoking package's name.
            package: env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "<unknown>".to_string()),
            disambiguator: crate::construct::crate_local_disambiguator(),
            tag: format!("{}", tag),
            data,
            crate_name: env::var("CARGO_CRATE_NAME").unwrap_or_else(|_| "<unknown>".to_string()),
        }
    }

    fn mangle(&self) -> String {
        format!(
            r#"{{"package":"{}","tag":"{}","data":"{}","disambiguator":"{}","crate_name":"{}"}}"#,
            json_escape(&self.package),
            json_escape(&self.tag),
            json_escape(self.data),
            self.disambiguator,
            json_escape(&self.crate_name),
        )
    }
}

fn json_escape(string: &str) -> String {
    let mut escaped = String::new();
    for c in string.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '\"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            c if c.is_control() || c == '@' => write!(escaped, "\\u{:04x}", c as u32).unwrap(),
            c => escaped.push(c),
        }
    }
    escaped
}
