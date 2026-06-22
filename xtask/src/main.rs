#![forbid(unsafe_code)]

use std::process::{Command, ExitCode};

use engine_core::{EngineError, EngineResult};
use engine_packager::{
    PackageChannel, PackageFormat, PackageRequest, PackageTarget, package_project,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> EngineResult<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("runtime-min") => build_profile(Profile::RuntimeMin, false),
        Some("build-editor") => build_profile(Profile::Editor, false),
        Some("agent-smoke") => cargo([
            "test",
            "-p",
            "engine-editor",
            "--no-default-features",
            "--features",
            "agent-tools",
        ]),
        Some("package") => package(args.collect()),
        Some("test") => cargo(["test", "--workspace"]),
        Some("check") => cargo(["check", "--workspace", "--all-features"]),
        Some(command) => Err(EngineError::config(format!(
            "unknown xtask command `{command}`"
        ))),
        None => Err(EngineError::config(
            "expected xtask command: runtime-min, build-editor, agent-smoke, package, test, or check",
        )),
    }
}

fn package(args: Vec<String>) -> EngineResult<()> {
    let mut project = std::path::PathBuf::from("examples/project");
    let mut target = PackageTarget::current_desktop();
    let mut format = PackageFormat::Folder;
    let mut channel = PackageChannel::Debug;
    let mut output_dir = None;
    let mut optimize_assets = true;
    let mut include_debug_symbols = false;

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--project" | "-p" => {
                let Some(value) = iter.next() else {
                    return Err(EngineError::config("--project requires a value"));
                };
                project = value.into();
            }
            "--target" => {
                let Some(value) = iter.next() else {
                    return Err(EngineError::config("--target requires a value"));
                };
                target = PackageTarget::parse(&value)?;
            }
            "--format" => {
                let Some(value) = iter.next() else {
                    return Err(EngineError::config("--format requires a value"));
                };
                format = PackageFormat::parse(&value)?;
            }
            "--channel" => {
                let Some(value) = iter.next() else {
                    return Err(EngineError::config("--channel requires a value"));
                };
                channel = PackageChannel::parse(&value)?;
            }
            "--release" => channel = PackageChannel::Release,
            "--debug" => channel = PackageChannel::Debug,
            "--output" | "-o" => {
                let Some(value) = iter.next() else {
                    return Err(EngineError::config("--output requires a value"));
                };
                output_dir = Some(value.into());
            }
            "--no-optimize-assets" => optimize_assets = false,
            "--include-debug-symbols" => include_debug_symbols = true,
            "--help" | "-h" => {
                print_package_help();
                return Ok(());
            }
            other => {
                return Err(EngineError::config(format!(
                    "unknown package argument `{other}`"
                )));
            }
        }
    }

    let output = package_project(&PackageRequest {
        project,
        repo_root: workspace_root()?,
        target,
        format,
        channel,
        optimize_assets,
        include_debug_symbols,
        output_dir,
    })?;
    println!("Packaged {}", output.project);
    println!("  target: {}", output.target);
    println!("  format: {}", output.format);
    println!("  channel: {}", output.channel);
    println!("  output: {}", output.path.display());
    if let Some(binary) = output.binary {
        println!("  binary: {}", binary.display());
    }
    if let Some(launcher) = output.launcher {
        println!("  launcher: {}", launcher.display());
    }
    println!(
        "  assets: {} ({})",
        output.asset_count,
        output.assets_manifest.display()
    );
    Ok(())
}

fn print_package_help() {
    println!(
        "usage: cargo xtask package [--project PATH] [--target TARGET] [--format FORMAT] [--debug|--release] [--output PATH]"
    );
    println!(
        "targets: native, linux-x64, windows-x64, macos-universal, android-arm64, ios-universal"
    );
    println!("formats: folder, apk, aab, ipa, appimage, deb, rpm, exe, msi, nsis, dmg");
}

fn workspace_root() -> EngineResult<std::path::PathBuf> {
    std::env::current_dir().map_err(|source| EngineError::Filesystem {
        path: ".".into(),
        source,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    RuntimeMin,
    Editor,
}

impl Profile {
    const fn name(self) -> &'static str {
        match self {
            Self::RuntimeMin => "runtime-min",
            Self::Editor => "editor",
        }
    }
}

fn build_profile(profile: Profile, release: bool) -> EngineResult<()> {
    let mut base_args = vec![
        "build",
        "-p",
        "runtime-min",
        "--no-default-features",
        "--features",
        profile.name(),
    ];
    if release {
        base_args.push("--release");
    }
    cargo_vec(&base_args)
}

fn cargo_vec(args: &[&str]) -> EngineResult<()> {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .map_err(|source| EngineError::Filesystem {
            path: "cargo".into(),
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(EngineError::other(format!("cargo exited with {status}")))
    }
}

fn cargo<const N: usize>(args: [&str; N]) -> EngineResult<()> {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .map_err(|source| EngineError::Filesystem {
            path: "cargo".into(),
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(EngineError::other(format!("cargo exited with {status}")))
    }
}
