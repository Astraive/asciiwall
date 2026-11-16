use std::process::ExitCode;
use std::sync::Arc;

use ascii_core::CancellationToken;

fn main() -> ExitCode {
    let cancellation = Arc::new(CancellationToken::new());
    let signal_cancellation = Arc::clone(&cancellation);
    if ctrlc::set_handler(move || signal_cancellation.cancel()).is_err() {
        eprintln!("cannot initialize conversion cancellation");
        return ExitCode::from(8);
    }
    asciiwall_cli::run_with_cancellation(std::env::args_os(), &cancellation)
}
