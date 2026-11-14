//! Logging setup (NFR-OPS-01): structured logs to journald, tagged with the
//! component name. Falls back to stderr when journald is not reachable
//! (tests, a plain terminal). The filter comes from `NEUROOS_LOG`.
//!
//! Rule R0-10: nothing personal is logged at `info` or above. That is a rule
//! for call sites; this module only routes records.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Environment variable holding the log filter (e.g. `info,neuroos_ipc=debug`).
pub const LOG_ENV: &str = "NEUROOS_LOG";

/// Default filter when `NEUROOS_LOG` is unset.
pub const DEFAULT_FILTER: &str = "info";

/// Errors installing the logger.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The filter text is invalid.
    #[error("invalid {LOG_ENV} filter: {0}")]
    Filter(String),
    /// A global subscriber was already installed.
    #[error("logger already initialised")]
    AlreadySet,
}

/// Where log records go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    /// The systemd journal.
    Journald,
    /// Standard error.
    Stderr,
}

/// Builds the filter from `NEUROOS_LOG`'s value (or the default).
///
/// # Errors
/// [`LoggingError::Filter`] for an unparsable filter.
pub fn filter(value: Option<&str>) -> Result<EnvFilter, LoggingError> {
    let text = value.filter(|v| !v.is_empty()).unwrap_or(DEFAULT_FILTER);
    EnvFilter::try_new(text).map_err(|e| LoggingError::Filter(e.to_string()))
}

/// Installs the global logger for `component` and returns the sink chosen.
///
/// # Errors
/// Bad filter, or a logger is already installed.
pub fn init(component: &str, log_env: Option<&str>) -> Result<Sink, LoggingError> {
    let filter = filter(log_env)?;
    match tracing_journald::layer() {
        Ok(journald) => {
            let journald = journald.with_syslog_identifier(component.to_owned());
            tracing_subscriber::registry()
                .with(filter)
                .with(journald)
                .try_init()
                .map_err(|_| LoggingError::AlreadySet)?;
            Ok(Sink::Journald)
        }
        Err(_) => {
            let fmt = tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true);
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt)
                .try_init()
                .map_err(|_| LoggingError::AlreadySet)?;
            Ok(Sink::Stderr)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_custom_filters_parse() {
        assert!(filter(None).is_ok());
        assert!(filter(Some("")).is_ok());
        assert!(filter(Some("warn,neuroos_ipc=debug")).is_ok());
    }

    #[test]
    fn bad_filter_is_rejected() {
        assert!(matches!(
            filter(Some("neuroos_ipc=notalevel")),
            Err(LoggingError::Filter(_))
        ));
    }

    #[test]
    fn second_init_fails() {
        let first = init("neuroos-test", Some("warn"));
        assert!(first.is_ok());
        assert!(matches!(
            init("neuroos-test", Some("warn")),
            Err(LoggingError::AlreadySet)
        ));
    }
}
