use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init_logging() {
    let stdout_layer = fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(std::io::stdout);

    tracing_subscriber::registry()
        .with(
            EnvFilter::from_default_env() // RUST_LOG controls overall verbosity
                .add_directive(Level::INFO.into()),
        )
        .with(stdout_layer)
        .init();
}
