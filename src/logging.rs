use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init_logging() {
    let stdout_layer = fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_ansi(true)
        .with_writer(std::io::stdout);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .init();
}
