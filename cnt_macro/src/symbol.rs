//! Symbol and section names of the markers emitted by the macros.
//!
//! Counters, layouts and instances are described to host tools through the names of marker symbols, which are JSON
//! objects (see `cnt_core::Symbol`). The markers themselves are placed in non-allocated (INFO) sections at address 0
//! by `cnt.x`, so that their addresses are dense indices.

use proc_macro2::TokenStream;
use quote::quote;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fmt::Write;
use std::hash::{Hash as _, Hasher as _};

/// Storage type used for a counter.
#[derive(Copy, Clone)]
pub(crate) enum Storage {
    /// For counters stored in RAM, reset on boot.
    Ram,
    /// For counters stored in non-volatile memory.
    Bkp,
}

impl Storage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Storage::Ram => "ram",
            Storage::Bkp => "bkp",
        }
    }

    /// Input section of counter markers, collected by `cnt.x` into `.counters_ram`/`.counters_bkp`.
    fn section(self) -> &'static str {
        match self {
            Storage::Ram => "cnt_ram",
            Storage::Bkp => "cnt_bkp",
        }
    }

    pub(crate) fn tokens(self) -> TokenStream {
        match self {
            Storage::Ram => quote!(Ram),
            Storage::Bkp => quote!(Bkp),
        }
    }
}

/// Numeric type of counter. Currently supported values: u32 and u64
#[derive(Copy, Clone)]
pub(crate) enum Ty {
    U32,
    U64,
}

impl Ty {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "u32" => Some(Ty::U32),
            "u64" => Some(Ty::U64),
            _ => None,
        }
    }

    /// Number of 32-bit words a counter of this type occupies.
    pub(crate) fn words(self) -> usize {
        match self {
            Ty::U32 => 1,
            Ty::U64 => 2,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Ty::U32 => "u32",
            Ty::U64 => "u64",
        }
    }

    pub(crate) fn tokens(self) -> TokenStream {
        match self {
            Ty::U32 => quote!(U32),
            Ty::U64 => quote!(U64),
        }
    }
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

impl Severity {
    pub(crate) const SUPPORTED: &str = "error, warn, info, debug, trace";

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "error" => Some(Severity::Error),
            "warn" => Some(Severity::Warn),
            "info" => Some(Severity::Info),
            "debug" => Some(Severity::Debug),
            "trace" => Some(Severity::Trace),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
            Severity::Debug => "debug",
            Severity::Trace => "trace",
        }
    }
}

/// Name, type, unit and severity of a counter, shared by call-site counters and `Count` variants.
pub(crate) struct Field<'a> {
    pub(crate) name: &'a str,
    pub(crate) ty: Ty,
    pub(crate) unit: &'a str,
    pub(crate) severity: Severity,
}

impl Field<'_> {
    fn json(&self) -> String {
        format!("{{{}}}", self.json_members())
    }

    /// JSON members without braces, so they can be merged into a larger object.
    fn json_members(&self) -> String {
        format!(
            r#""name":"{}","ty":"{}","unit":"{}","severity":"{}""#,
            json_escape(self.name),
            self.ty.as_str(),
            json_escape(self.unit),
            self.severity.as_str()
        )
    }
}

/// Where a macro was invoked, and what disambiguates otherwise identical invocations.
pub(crate) struct CallSite {
    /// Name of the Cargo package in which the symbol is being instantiated.
    package: String,
    /// Crate name obtained via CARGO_CRATE_NAME (added since a Cargo package can contain many crates).
    crate_name: String,
    file: String,
    line: usize,
    column: usize,
    /// Hash of the call site span, in case file, line and column are not unique (macro-generated invocations).
    id: u64,
}

impl CallSite {
    pub(crate) fn here() -> Self {
        let span = proc_macro::Span::call_site();
        Self {
            package: env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "<unknown>".to_string()),
            crate_name: env::var("CARGO_CRATE_NAME").unwrap_or_else(|_| "<unknown>".to_string()),
            file: span.file(),
            line: span.line(),
            column: span.column(),
            id: hash(&format!("{span:?}")),
        }
    }

    /// Trailing JSON members (without braces), common to all marker kinds.
    fn json(&self) -> String {
        format!(
            r#""package":"{}","crate":"{}","file":"{}","line":{},"column":{},"id":"{:x}""#,
            json_escape(&self.package),
            json_escape(&self.crate_name),
            json_escape(&self.file),
            self.line,
            self.column,
            self.id
        )
    }
}

/// Symbol name of a call-site counter marker, placed in `.counters_ram`/`.counters_bkp`.
pub(crate) fn counter_symbol(field: &Field, group: &str, site: &CallSite) -> String {
    format!(
        r#"{{"kind":"counter","group":"{}",{},{}}}"#,
        json_escape(group),
        field.json_members(),
        site.json()
    )
}

/// Symbol name of a layout marker, placed in `.cnt_layout`. Describes the counters of a `Count` type.
pub(crate) fn layout_symbol(ty: &str, fields: &[Field], site: &CallSite) -> String {
    let fields: Vec<String> = fields.iter().map(Field::json).collect();
    format!(
        r#"{{"kind":"layout","ty":"{}","fields":[{}],{}}}"#,
        json_escape(ty),
        fields.join(","),
        site.json()
    )
}

/// Symbol names of an instance's slots marker (in `.counters_ram`/`.counters_bkp`) and info marker (in
/// `.cnt_instance`).
pub(crate) fn instance_symbols(name: &str, storage: Storage, site: &CallSite) -> (String, String) {
    let rest = format!(
        r#""name":"{}","storage":"{}",{}}}"#,
        json_escape(name),
        storage.as_str(),
        site.json()
    );
    (
        format!(r#"{{"kind":"slots",{rest}"#),
        format!(r#"{{"kind":"instance",{rest}"#),
    )
}

/// Attributes placing a marker static into `.<section>.<symbol>` and exporting it as `symbol`.
///
/// Works around restrictions on length and allowed characters imposed by the macOS linker (relevant for host builds
/// only): there the subsection is a 16 character hex digest of the symbol.
pub(crate) fn marker_attrs(section: &str, symbol: &str) -> TokenStream {
    let elf = format!(".{section}.{symbol}");
    let macos = format!(".{section},{:x}", hash(symbol));
    quote! {
        #[used]
        #[cfg_attr(target_os = "macos", unsafe(link_section = #macos))]
        #[cfg_attr(not(target_os = "macos"), unsafe(link_section = #elf))]
        #[unsafe(export_name = #symbol)]
    }
}

/// Attributes of a counter marker, see [`marker_attrs`].
pub(crate) fn counter_marker_attrs(storage: Storage, symbol: &str) -> TokenStream {
    marker_attrs(storage.section(), symbol)
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

fn hash(string: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    string.hash(&mut hasher);
    hasher.finish()
}
