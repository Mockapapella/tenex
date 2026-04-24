//! Probe mux client daemon startup paths from a non-test binary build.
//!
//! This binary supports being invoked as `... muxd` so it can act as a self-hosted mux daemon
//! when `mux::client` resolves the current executable to `tenex` and spawns `tenex muxd`.

use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("muxd") {
        if let Err(err) = tenex::mux::run_mux_daemon() {
            eprintln!("{err:#}");
            std::process::exit(1);
        }
        return;
    }

    let session_name = match std::env::var("TENEX_TEST_MUX_SESSION_NAME") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("TENEX_TEST_MUX_SESSION_NAME is required");
            std::process::exit(2);
        }
    };

    let workdir = match std::env::var("TENEX_TEST_MUX_WORKDIR") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("TENEX_TEST_MUX_WORKDIR is required");
            std::process::exit(2);
        }
    };

    let session_manager = tenex::mux::SessionManager::new();
    if let Err(err) = session_manager.create(&session_name, Path::new(&workdir), None) {
        eprintln!("{err:#}");
        std::process::exit(1);
    }

    let _ = session_manager.kill(&session_name);
}
