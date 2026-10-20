//! One Worker's browser execution boundary. No product Store or node routing.
//!
//! The verified MCP executor mints invocation authority; thin stdio MCP only
//! forwards it. Every backend dispatch must pass the same epoch/input gate.
mod action;
mod driver;
#[cfg(unix)]
mod ipc;
mod managed;
mod pressed;
mod process;
mod proxy;
mod rfb;
mod runtime;
pub use managed::ManagedBrowser;
mod stdio;

pub use action::{BrowserAction, BrowserActionExt, BrowserTabAction};
pub use runtime::desktop::{DesktopConnector, DesktopTransport};
pub use runtime::{
    BrowserBackend, BrowserDispatch, BrowserFuture, BrowserInvocationIdentity,
    BrowserInvocationLease, BrowserRuntime, PluginEnabledReader,
};
pub use stdio::run_stdio;

pub const INVOCATION_META_KEY: &str = "talosBrowserInvocation";
pub const SOCKET_ENV: &str = "TALOS_BROWSER_SOCKET";

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

mod native;
pub use native::{BrowserHostPort, NativeBrowser};

mod files;
