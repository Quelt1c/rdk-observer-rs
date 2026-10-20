//! Executable entry point for RDK Observer.
//!
//! Keep worker orchestration in the library's `runtime` module. Argument parsing,
//! configuration loading, and logging initialization will be added here before
//! handing control to the runtime once those interfaces are defined.

use std::process::ExitCode;

use rdk_observer_core::runtime;

fn main() -> ExitCode {
    // The runtime owns worker startup, observer modes, and shutdown.
    match runtime::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rdk-observer: {error}");
            ExitCode::FAILURE
        }
    }
}
