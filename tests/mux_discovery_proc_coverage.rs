//! Cover linux `/proc` socket discovery branches from a non-test build.

#[cfg(target_os = "linux")]
mod linux_only {
    use std::collections::HashSet;
    use std::process::Command;
    use std::time::Duration;

    struct ChildGuard(std::process::Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn test_running_mux_sockets_skips_empty_socket_env_in_non_test_build()
    -> Result<(), Box<dyn std::error::Error>> {
        let dummy = ChildGuard(
            Command::new("bash")
                .args(["-c", "exec -a muxd sleep 60"])
                .env("TENEX_MUX_SOCKET", " ")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?,
        );

        let pid = dummy.0.id();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"));
            if let Ok(cmdline) = cmdline
                && cmdline
                    .split(|b| *b == 0)
                    .filter(|arg| !arg.is_empty())
                    .any(|arg| arg == b"muxd")
            {
                break;
            }
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))?;
        assert!(
            cmdline
                .split(|b| *b == 0)
                .filter(|arg| !arg.is_empty())
                .any(|arg| arg == b"muxd"),
            "expected dummy process to retain muxd argv[0]"
        );

        let empty_sessions = HashSet::new();
        assert!(tenex::mux::discover_socket_for_sessions(&empty_sessions, None).is_none());

        let mut wanted_sessions = HashSet::new();
        wanted_sessions.insert("tenex-test-nonexistent-session".to_string());
        assert!(tenex::mux::discover_socket_for_sessions(&wanted_sessions, None).is_none());
        assert!(
            tenex::mux::discover_socket_for_sessions(
                &wanted_sessions,
                Some("tenex-missing-socket")
            )
            .is_none()
        );
        assert!(tenex::mux::discover_socket_for_sessions(&wanted_sessions, Some(" ")).is_none());

        Ok(())
    }
}
