use std::error::Error;
use std::fmt;

use talos_error::TalosError;

/// Report a handled product failure once at its request, terminal, or task owner.
///
/// Propagation and historical replay must not report again. Sources must already
/// exclude credentials and raw sensitive bodies at their dependency boundary.
/// The current tracing span supplies request/turn correlation; this function
/// neither selects public messages nor owns lifecycle or transport policy.
pub fn report_error(error: &TalosError) {
    macro_rules! report {
        ($level:expr) => {
            tracing::event!(
                target: "talos::diagnostic",
                $level,
                kind = if error.is_internal() { "Internal" } else { "Business" },
                code = ?error.error_code(),
                public_message = %error.to_api_error().message,
                context = error.diagnostic_message(),
                sources = %ErrorSources(error),
                location = %error.location(),
                backtrace_status = ?error.backtrace().status(),
                backtrace = %error.backtrace(),
                "application failure"
            )
        };
    }
    if error.is_internal() {
        report!(tracing::Level::ERROR)
    } else {
        report!(tracing::Level::WARN)
    }
}

struct ErrorSources<'a>(&'a TalosError);

impl fmt::Display for ErrorSources<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut source = self.0.source();
        let mut visited = Vec::new();
        while let Some(current) = source {
            if visited
                .iter()
                .any(|previous| std::ptr::addr_eq(*previous, current))
            {
                return formatter.write_str("[source cycle omitted]");
            }
            if visited.len() == 64 {
                return formatter.write_str("[source chain truncated at 64 entries]");
            }
            writeln!(formatter, "{}: {current}", visited.len())?;
            visited.push(current);
            source = current.source();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "error_report_tests.rs"]
mod tests;
