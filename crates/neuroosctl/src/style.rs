//! Terminal styling and color handling (design.md §7).
//!
//! Respects `NO_COLOR`, `--color auto|always|never`, and non-TTY stdout/stderr.

use std::io::IsTerminal;

use clap::ValueEnum;

/// Color choice for terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ColorChoice {
    /// Color if stdout is a TTY and `NO_COLOR` is unset.
    #[default]
    Auto,
    /// Always use ANSI colors.
    Always,
    /// Never use ANSI colors.
    Never,
}

/// Output formatter with optional ANSI colors.
#[derive(Debug, Clone, Copy)]
pub struct Styler {
    enabled: bool,
}

impl Styler {
    /// Creates a styler evaluating the given mode against TTY status and `NO_COLOR`.
    #[must_use]
    pub fn new(choice: ColorChoice) -> Self {
        let is_tty = std::io::stdout().is_terminal();
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let enabled = match choice {
            ColorChoice::Never => false,
            ColorChoice::Always => true,
            ColorChoice::Auto => is_tty && !no_color,
        };
        Self { enabled }
    }

    /// Whether color is active.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Green text (e.g. `✔ OK`).
    #[must_use]
    pub fn ok(&self, text: &str) -> String {
        if self.enabled {
            format!("\x1b[32m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Yellow text (e.g. `▲ DEGRADED`).
    #[must_use]
    pub fn degraded(&self, text: &str) -> String {
        if self.enabled {
            format!("\x1b[33m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Red text (e.g. `✖ DOWN`).
    #[must_use]
    pub fn down(&self, text: &str) -> String {
        if self.enabled {
            format!("\x1b[31m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Bold text (table headers).
    #[must_use]
    pub fn bold(&self, text: &str) -> String {
        if self.enabled {
            format!("\x1b[1m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Dimmed text (subtle labels, UNKNOWN status).
    #[must_use]
    pub fn dim(&self, text: &str) -> String {
        if self.enabled {
            format!("\x1b[2m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Formats an error line: `error: <msg>`.
    #[must_use]
    pub fn error(&self, msg: &str) -> String {
        if self.enabled {
            format!("\x1b[31;1merror:\x1b[0m {msg}")
        } else {
            format!("error: {msg}")
        }
    }

    /// Formats a hint line: `hint: <msg>`.
    #[must_use]
    pub fn hint(&self, msg: &str) -> String {
        if self.enabled {
            format!("\x1b[2mhint:\x1b[0m {msg}")
        } else {
            format!("hint: {msg}")
        }
    }
}
