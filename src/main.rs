#![deny(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use strictsh::capability::ExitCode;

fn main() -> ExitCode {
    match strictsh::capability::run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("strictsh: {error}");
            ExitCode::from(2)
        }
    }
}
