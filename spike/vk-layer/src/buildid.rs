//! Build identity, compiled in by `build.sh`.
//!
//! Round 1 twice mistook a stale build for a behavioural result, because the
//! manifest points at the library by absolute path: "cargo finished" does not
//! imply "this is the code that ran". The layer therefore prints this identity
//! from `vkNegotiateLoaderLayerInterfaceVersion`, and `run.sh` prints the
//! SHA-256 and mtime of the `.so` that `/proc/<pid>/maps` says is mapped.
//!
//! `option_env!` rather than a generated file: a plain `cargo build` outside
//! `build.sh` still compiles (and says `unbuilt`), and no build ever dirties
//! the working tree.

pub const BUILD_ID: &str = match option_env!("LAPSPHERE_BUILD_ID") {
    Some(v) => v,
    None => "unbuilt",
};

pub const BUILD_MTIME: &str = match option_env!("LAPSPHERE_BUILD_MTIME") {
    Some(v) => v,
    None => "unbuilt",
};

/// The repository commit the sources came from. The report ties every result
/// to this value.
pub const GIT_HEAD: &str = match option_env!("LAPSPHERE_GIT_HEAD") {
    Some(v) => v,
    None => "unbuilt",
};
