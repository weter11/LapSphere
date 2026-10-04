//! The session-bus connection that owns `io.lapsphere.Gui`.
//!
//! Why this exists: an object exported on one `zbus::Connection` is only
//! reachable through THAT connection's name. The single-instance guard requests
//! `io.lapsphere.Gui` on its own connection; exporting the panel object on a
//! second, freshly-created connection therefore put it on that connection's
//! unique name, where nothing could ever call it. Introspection of
//! `io.lapsphere.Gui` returned an empty tree and every call timed out.
//!
//! So the owning connection is parked here before `run_native`, and the panel
//! control object is exported on it.

use std::sync::OnceLock;

static CONNECTION: OnceLock<Option<zbus::Connection>> = OnceLock::new();

/// Park the session-bus connection that owns `io.lapsphere.Gui`.
pub fn set_connection(connection: Option<zbus::Connection>) {
    let _ = CONNECTION.set(connection);
}

/// The owning connection, if one was parked.
pub fn connection() -> Option<zbus::Connection> {
    CONNECTION.get().and_then(|conn| conn.clone())
}
