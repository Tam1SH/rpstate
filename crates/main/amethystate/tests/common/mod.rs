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
#[allow(dead_code)]
pub fn per_engine(name: &str) -> String {
    format!(
        "{name}_{}",
        amethystate::store::builder::default_backend().extension()
    )
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
