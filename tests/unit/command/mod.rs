use anyhow::Result;
use tenex::test_support::parse_command_line;

fn error_message<T, E: std::fmt::Display>(
    result: std::result::Result<T, E>,
    context: &str,
) -> Result<String> {
    match result {
        Ok(_) => anyhow::bail!("{context}"),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn test_parse_command_line_rejects_empty() -> Result<()> {
    let message = error_message(parse_command_line("   "), "expected empty command error")?;
    assert!(message.contains("Command line is empty"));
    Ok(())
}

#[test]
fn test_parse_command_line_splits_args() -> Result<()> {
    let argv = parse_command_line(r#"echo "hello world""#)?;
    assert_eq!(argv, vec!["echo".to_string(), "hello world".to_string()]);
    Ok(())
}

#[test]
fn test_parse_command_line_comment_is_empty() -> Result<()> {
    let message = error_message(
        parse_command_line("# comment only"),
        "expected comment error",
    )?;
    assert!(message.contains("Command line produced no argv items"));
    Ok(())
}
