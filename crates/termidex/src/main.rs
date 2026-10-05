//! The `termidex` binary. All the work lives in the library so the tests can reach it.

use std::process::ExitCode;

fn main() -> ExitCode {
    termidex::app::run()
}
