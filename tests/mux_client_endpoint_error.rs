//! Coverage for mux client behavior when endpoint resolution fails.

use anyhow::Result;
use tenex::mux::{SessionManager, set_socket_override};

#[test]
fn test_mux_client_request_errors_when_endpoint_resolution_fails() -> Result<()> {
    set_socket_override("/tmp/tenex-mux-test\0bad.sock")?;
    let result = SessionManager::new().list();
    assert!(result.is_err());
    Ok(())
}
