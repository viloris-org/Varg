#![forbid(unsafe_code)]

use std::process::ExitCode;

use engine_core::{EngineError, EngineResult, RuntimeProfile};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("engine-cli error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> EngineResult<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("smoke") => smoke(args.next())?,
        Some("profiles") => print_profiles(),
        Some("--help") | Some("-h") | Some("help") => print_help(),
        Some(command) => {
            return Err(EngineError::config(format!(
                "unknown engine-cli command `{command}`"
            )));
        }
    }

    Ok(())
}

fn smoke(profile_arg: Option<String>) -> EngineResult<()> {
    let profile = match profile_arg.as_deref() {
        None => RuntimeProfile::RuntimeMin,
        Some("runtime-min") => RuntimeProfile::RuntimeMin,
        Some("runtime-game") => RuntimeProfile::RuntimeGame,
        Some("editor") => RuntimeProfile::Editor,
        Some("agent-tools") => RuntimeProfile::AgentTools,
        Some("script-python") => RuntimeProfile::ScriptPython,
        Some("dev-full") => RuntimeProfile::DevFull,
        Some(profile) => {
            return Err(EngineError::config(format!(
                "unsupported profile `{profile}`"
            )));
        }
    };

    let frame = runtime_min::smoke_runtime_min()?;
    println!(
        "Aster {} smoke completed at frame {frame}",
        profile.as_str()
    );
    Ok(())
}

fn print_profiles() {
    for profile in [
        RuntimeProfile::RuntimeMin,
        RuntimeProfile::RuntimeGame,
        RuntimeProfile::Editor,
        RuntimeProfile::AgentTools,
        RuntimeProfile::ScriptPython,
        RuntimeProfile::DevFull,
    ] {
        println!("{}", profile.as_str());
    }
}

fn print_help() {
    println!("Aster native CLI");
    println!();
    println!("Usage:");
    println!("  cargo run -p engine-cli -- [smoke] [profile]");
    println!("  cargo run -p engine-cli -- profiles");
}
