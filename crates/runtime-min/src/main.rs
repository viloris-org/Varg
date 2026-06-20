#![forbid(unsafe_code)]

use std::path::PathBuf;

use engine_core::EngineResult;

fn main() {
    if let Err(error) = run() {
        eprintln!("aster runtime error: {error}");
        std::process::exit(1);
    }
}

fn run() -> EngineResult<()> {
    let project = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("project"));
    runtime_min::run_project(project)
}
