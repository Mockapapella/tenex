//! Minimal driver binary that exercises the legacy state-dir migration.

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::sink)
        .try_init();

    if let Err(err) = tenex::migration::migrate_default_state_dir() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
