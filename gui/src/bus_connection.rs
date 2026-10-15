//! The session-bus connection that owns `io.lapsphere.Gui`.
//!
//! Why this exists: an object exported on one `zbus::Connection` is reachable
//! only through THAT connection's name. The single-instance guard requests
//! `io.lapsphere.Gui` on a connection of its own, so exporting the panel's
//! control object on a second, freshly created connection would put the object
//! on the second connection's unique name — where nothing can call it.
//! `busctl --user tree io.lapsphere.Gui` returns an empty tree, `introspect`
//! times out, and `lapsphere --toggle-panel` hangs, all while the process owning
//! the name looks perfectly healthy.
//!
//! So the owning connection is parked here before `run_native`, and the panel
//! control object is exported on it.

use std::sync::OnceLock;

/// The GUI's session-bus name: the single-instance guard's, and the one the
/// panel's `TogglePanel` method is exported under.
pub const GUI_BUS_NAME: &str = "io.lapsphere.Gui";

static CONNECTION: OnceLock<Option<zbus::Connection>> = OnceLock::new();

/// Park the session-bus connection that owns [`GUI_BUS_NAME`].
///
/// Called once, before the GUI starts. `None` records that there is no session
/// bus — a state the panel has to report rather than discover per call.
pub fn set_connection(connection: Option<zbus::Connection>) {
    let _ = CONNECTION.set(connection);
}

/// The owning connection, if one was parked.
///
/// `None` means no session bus, which is a state the panel reports rather than
/// discovers per call: with no connection there is no control object, so the CLI
/// is not a way back and the safety rule must not count it.
pub fn connection() -> Option<zbus::Connection> {
    CONNECTION.get().and_then(|conn| conn.clone())
}
