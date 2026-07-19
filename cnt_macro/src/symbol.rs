use std::env;
// Borrowed from defmt
use std::fmt::Write;

pub(crate) fn mangled(
    group: &str,
    storage: Storage,
    name: &str,
    ty: Ty,
    unit: &str,
    severity: Severity,
) -> String {
    Symbol::new(group, storage, name, ty, unit, severity).mangle()
}

struct Symbol<'a> {
    /// Name of the Cargo package in which the symbol is being instantiated. Used for avoiding
    /// symbol name collisions.
    package: String,

    /// User tag of a counter can be used to group counters together.
    group: &'a str,

    storage: Storage,

    /// Name of a counter.
    name: &'a str,

    ty: Ty,

    /// Optional unit of a counter.
    unit: &'a str,

    severity: Severity,

    /// Unique identifier that disambiguates otherwise equivalent invocations in the same crate.
    disambiguator: u64,

    /// Crate name obtained via CARGO_CRATE_NAME (added since a Cargo package can contain many crates).
    crate_name: String,
}

/// Storage type used for a counter.
#[derive(Copy, Clone)]
pub(crate) enum Storage {
    /// For counters stored in RAM, reset on boot.
    RAM,
    /// For counters stored in non-volatile memory.
    BKP,
}

/// Numeric type of counter. Currently supported values: u32 and u64
#[derive(Copy, Clone)]
pub(crate) enum Ty {
    U32,
    U64,
}

/// Severity of a counter. Currently supported values: error, warn, info, debug, trace
#[derive(Copy, Clone)]
pub(crate) enum Severity {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl<'a> Symbol<'a> {
    fn new(
        group: &'a str,
        storage: Storage,
        name: &'a str,
        ty: Ty,
        unit: &'a str,
        severity: Severity,
    ) -> Self {
        Self {
            // `CARGO_PKG_NAME` is set to the invoking package's name.
            package: env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "<unknown>".to_string()),
            disambiguator: crate::construct::crate_local_disambiguator(),
            storage,
            name,
            ty,
            unit,
            severity,
            group,
            crate_name: env::var("CARGO_CRATE_NAME").unwrap_or_else(|_| "<unknown>".to_string()),
        }
    }

    fn mangle(&self) -> String {
        format!(
            r#"{{"package":"{}","group":"{}","storage":"{}","name":"{}","ty":"{}","unit":"{}","severity":"{}","disambiguator":"{}","crate_name":"{}"}}"#,
            json_escape(&self.package),
            json_escape(self.group),
            self.storage.as_str(),
            json_escape(self.name),
            self.ty.as_str(),
            json_escape(self.unit),
            self.severity.as_str(),
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

impl Storage {
    fn as_str(&self) -> &'static str {
        match self {
            Storage::RAM => "cnt_ram",
            Storage::BKP => "cnt_bkp",
        }
    }
}

impl Ty {
    fn as_str(&self) -> &'static str {
        match self {
            Ty::U32 => "u32",
            Ty::U64 => "u64",
        }
    }
}

impl Severity {
    fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
            Severity::Debug => "debug",
            Severity::Trace => "trace",
        }
    }
}
