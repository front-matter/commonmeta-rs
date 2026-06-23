use std::io::IsTerminal;

use indicatif::{ProgressBar, ProgressStyle};

const COUNT_TEMPLATE: &str = "{prefix} {bar:40.cyan/blue} {pos}/{len} ({eta})";
const BYTES_TEMPLATE: &str =
    "{prefix} {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, {eta})";

/// A progress bar over a known number of items (e.g. records to convert or
/// render). Renders to stderr, and is a no-op when stderr isn't a terminal
/// (redirected to a file, piped, CI, etc.) so non-interactive output stays
/// clean.
pub fn count_bar(prefix: &str, total: u64) -> ProgressBar {
    if !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template(COUNT_TEMPLATE)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-"),
    );
    bar.set_prefix(prefix.to_string());
    bar
}

/// A progress bar over a known number of bytes (e.g. a file download).
/// Same terminal-detection behavior as [`count_bar`].
pub fn bytes_bar(prefix: &str, total_bytes: u64) -> ProgressBar {
    if !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::new(total_bytes);
    bar.set_style(
        ProgressStyle::with_template(BYTES_TEMPLATE)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-"),
    );
    bar.set_prefix(prefix.to_string());
    bar
}
