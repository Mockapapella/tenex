"""Logger smoke tests."""

from rich.console import Console

from tenex_cli.logger import Logger


def test_logger_methods_emit_expected_tags() -> None:
    """Calling each log helper should emit bracketed tags."""
    console = Console(record=True)
    logger = Logger(console=console)

    logger.info("hello")
    logger.warning("warn")
    logger.warning("warn2")
    logger.error("boom")

    output = console.export_text()
    expected_warns = 2
    assert "[INFO]" in output
    assert output.count("[WARN]") == expected_warns
    assert "[ERROR]" in output
