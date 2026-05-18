//! Probe `mux::running_daemon_version` in a non-test binary build.

use tenex::mux;

fn main() {
    if let Some(value) = std::env::var_os("TENEX_TEST_SOCKET_OVERRIDE_VALUE")
        && let Err(err) = mux::set_socket_override(&value.to_string_lossy())
    {
        eprintln!("{err:#}");
        std::process::exit(1);
    }

    if std::env::var_os("TENEX_TEST_CALL_IS_SERVER_RUNNING").is_some() {
        let _ = mux::is_server_running();
        if std::env::var_os("TENEX_TEST_EXIT_AFTER_IS_SERVER_RUNNING").is_some() {
            return;
        }
    }

    match mux::running_daemon_version() {
        Ok(Some(version)) => {
            print!("{version}");
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("{err:#}");
            std::process::exit(1);
        }
    }
}
