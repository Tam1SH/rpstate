//! Turning a failure into something a snapshot can hold.
//!
//! A report is a tree of contexts with attachments hanging off each frame, and
//! asserting on the outermost variant leaves the rest unchecked - which is how
//! a report that was correctly typed and said nothing useful got past a test
//! twice. What is pinned here is the whole shape.
//!
//! Three things another machine would spell differently are taken out: the
//! source location error-stack records for every frame, which moves whenever
//! the file above it does; the store's path, which is an absolute path into a
//! temporary directory and so differs by user and by platform; and the
//! backtrace, which error-stack captures whenever `RUST_BACKTRACE` is set - as
//! it usually is on CI and usually is not locally.

#![allow(dead_code)]

use std::sync::Once;

static COLOR: Once = Once::new();

pub fn shape<C>(report: &error_stack::Report<C>) -> String {
    COLOR.call_once(|| {
        error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);
    });

    let rendered = format!("{report:?}");

    rendered
        .lines()
        .take_while(|line| !starts_the_backtrace_section(line))
        .filter(|line| !is_source_location(line) && !is_backtrace_marker(line))
        .map(normalise)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// A snapshot name carrying the engine, for a report the engine has a hand in
/// writing - anything the codec or the file layout speaks through.
pub fn per_engine(name: &str) -> String {
    format!(
        "{name}_{}",
        amethystate::store::builder::default_backend().extension()
    )
}

/// The document engine a test about documents must name: the first of json,
/// toml, ron, which is the order the seeded text is chosen in too.
///
/// A store built without naming its backend takes
/// [`default_backend`](amethystate::store::builder::default_backend), and that
/// prefers redb. A test that seeds a json file and then opens a store without
/// saying so reads a redb database beside it, and what it asserts is about
/// redb.
#[cfg(any(feature = "json", feature = "toml", feature = "ron"))]
pub fn text_backend() -> amethystate::store::builder::Backend {
    use amethystate::store::builder::Backend;

    #[cfg(feature = "json")]
    {
        Backend::Json
    }
    #[cfg(all(feature = "toml", not(feature = "json")))]
    {
        Backend::Toml
    }
    #[cfg(all(feature = "ron", not(feature = "json"), not(feature = "toml")))]
    {
        Backend::Ron
    }
}

/// Every document engine the build enabled, for a question whose answer is the
/// difference between them. The features are additive, so a run with all three
/// on answers for all three; [`text_backend`] picks one and is for a test that
/// only needs a file it can read.
#[cfg(any(feature = "json", feature = "toml", feature = "ron"))]
pub fn text_backends() -> Vec<amethystate::store::builder::Backend> {
    use amethystate::store::builder::Backend;

    let mut enabled = Vec::new();
    #[cfg(feature = "json")]
    enabled.push(Backend::Json);
    #[cfg(feature = "ron")]
    enabled.push(Backend::Ron);
    #[cfg(feature = "toml")]
    enabled.push(Backend::Toml);
    enabled
}

/// One measurement, as fields rather than as a rendering of them.
///
/// A probe measures; how the book lays the result out is `cargo xtask docs`'s
/// business, and keeping the two apart is what stops a change of layout from
/// being a change to a test. The line is JSON so a value carrying newlines,
/// quotes or a pipe needs no escaping convention of its own.
pub fn measured(fields: &[(&str, &str)]) {
    let object: Vec<String> = fields
        .iter()
        .map(|(name, value)| format!("{}:{}", quoted(name), quoted(value)))
        .collect();

    println!("@measured {{{}}}", object.join(","));
}

fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');

    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }

    out.push('"');
    out
}

fn content(line: &str) -> &str {
    line.trim_start_matches(['│', '├', '╰', '╴', '─', '▶', ' '])
}

fn is_source_location(line: &str) -> bool {
    content(line).starts_with("at ")
}

fn is_backtrace_marker(line: &str) -> bool {
    content(line).starts_with("backtrace (")
}

/// Everything from here down is the captured backtrace, which error-stack
/// separates with a rule of its own.
fn starts_the_backtrace_section(line: &str) -> bool {
    line.starts_with('━') || line.starts_with("backtrace no.")
}

/// An attachment naming a file is `<label>: <absolute path>`, and only the
/// label carries meaning across machines.
fn normalise(line: &str) -> String {
    if !line.contains("amethystate-") {
        return line.to_string();
    }

    match line.rfind(": ") {
        Some(at) => format!("{}: <store>", &line[..at]),
        None => "<store>".to_string(),
    }
}
