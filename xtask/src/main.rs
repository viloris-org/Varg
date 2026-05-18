#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use engine_core::{EngineError, EngineResult};

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
        Some("runtime-min") => build_profile(Profile::RuntimeMin),
        Some("build-editor") => build_profile(Profile::Editor),
        Some("agent-smoke") => cargo([
            "test",
            "-p",
            "engine-editor",
            "--no-default-features",
            "--features",
            "agent-tools",
        ]),
        Some("package") => {
            let profile = parse_package_profile(args)?;
            package(profile)
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    RuntimeMin,
    RuntimeGame,
    Editor,
    AgentTools,
    ScriptPython,
    DevFull,
}

impl Profile {
    const fn name(self) -> &'static str {
        match self {
            Self::RuntimeMin => "runtime-min",
            Self::RuntimeGame => "runtime-game",
            Self::Editor => "editor",
            Self::AgentTools => "agent-tools",
            Self::ScriptPython => "script-python",
            Self::DevFull => "dev-full",
        }
    }

    fn parse(input: &str) -> EngineResult<Self> {
        match input {
            "runtime-min" => Ok(Self::RuntimeMin),
            "runtime-game" => Ok(Self::RuntimeGame),
            "editor" => Ok(Self::Editor),
            "agent-tools" => Ok(Self::AgentTools),
            "script-python" => Ok(Self::ScriptPython),
            "dev-full" => Ok(Self::DevFull),
            profile => Err(EngineError::config(format!(
                "unsupported profile `{profile}`"
            ))),
        }
    }
}

fn parse_package_profile(mut args: impl Iterator<Item = String>) -> EngineResult<Profile> {
    match (args.next().as_deref(), args.next()) {
        (Some("--profile"), Some(profile)) => Profile::parse(&profile),
        (Some(profile), None) => Profile::parse(profile),
        _ => Err(EngineError::config(
            "expected package --profile <runtime-game|editor>",
        )),
    }
}

fn build_profile(profile: Profile) -> EngineResult<()> {
    cargo([
        "build",
        "-p",
        "runtime-min",
        "--no-default-features",
        "--features",
        profile.name(),
    ])?;

    match profile {
        Profile::Editor | Profile::AgentTools | Profile::DevFull => cargo([
            "build",
            "-p",
            "engine-cli",
            "--no-default-features",
            "--features",
            profile.name(),
        ]),
        _ => Ok(()),
    }
}

fn package(profile: Profile) -> EngineResult<()> {
    match profile {
        Profile::RuntimeGame | Profile::Editor => {}
        unsupported => {
            return Err(EngineError::config(format!(
                "native packaging is supported for runtime-game and editor, got `{}`",
                unsupported.name()
            )));
        }
    }

    build_profile(profile)?;
    cargo([
        "build",
        "-p",
        "engine-cli",
        "--no-default-features",
        "--features",
        profile.name(),
    ])?;

    let package_root = PathBuf::from("target")
        .join("aster-packages")
        .join(platform_tag())
        .join(profile.name());
    recreate_dir(&package_root)?;

    let bin_dir = package_root.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|source| EngineError::Filesystem {
        path: bin_dir.clone(),
        source,
    })?;

    let cli_source = PathBuf::from("target")
        .join("debug")
        .join(executable_name("engine-cli"));
    let cli_dest = bin_dir.join(executable_name("aster"));
    fs::copy(&cli_source, &cli_dest).map_err(|source| EngineError::Filesystem {
        path: cli_source.clone(),
        source,
    })?;

    if profile == Profile::Editor {
        create_editor_app_layout(&package_root, &cli_dest)?;
    }

    write_manifest(&package_root, profile)?;
    write_marker_artifact(&package_root, profile)?;
    println!("packaged {} at {}", profile.name(), package_root.display());
    Ok(())
}

fn create_editor_app_layout(package_root: &Path, cli_dest: &Path) -> EngineResult<()> {
    let app_dir = package_root.join(editor_app_dir());
    let executable_dir = app_dir.join(editor_executable_dir());
    fs::create_dir_all(&executable_dir).map_err(|source| EngineError::Filesystem {
        path: executable_dir.clone(),
        source,
    })?;

    let editor_binary = executable_dir.join(executable_name("AsterEditor"));
    fs::copy(cli_dest, &editor_binary).map_err(|source| EngineError::Filesystem {
        path: cli_dest.to_path_buf(),
        source,
    })?;

    Ok(())
}

fn write_manifest(package_root: &Path, profile: Profile) -> EngineResult<()> {
    let manifest = format!(
        "name = \"Aster\"\nprofile = \"{}\"\nplatform = \"{}\"\ncli = \"bin/{}\"\n",
        profile.name(),
        platform_tag(),
        executable_name("aster")
    );
    let path = package_root.join("package.toml");
    fs::write(&path, manifest).map_err(|source| EngineError::Filesystem { path, source })
}

fn write_marker_artifact(package_root: &Path, profile: Profile) -> EngineResult<()> {
    let path = package_root.join(format!(
        "aster-{}-{}.{}",
        profile.name(),
        platform_tag(),
        package_extension()
    ));
    fs::write(
        &path,
        format!(
            "Aster native package marker\nprofile={}\nplatform={}\n",
            profile.name(),
            platform_tag()
        ),
    )
    .map_err(|source| EngineError::Filesystem { path, source })
}

fn recreate_dir(path: &Path) -> EngineResult<()> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;
    }
    fs::create_dir_all(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

const fn platform_tag() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else if cfg!(target_os = "macos") {
        "macos-x64"
    } else if cfg!(target_os = "linux") {
        "linux-x64"
    } else {
        "unknown"
    }
}

const fn package_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "zip"
    } else if cfg!(target_os = "macos") {
        "dmg"
    } else {
        "tar.gz"
    }
}

const fn editor_app_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "AsterEditor.app"
    } else {
        "AsterEditor"
    }
}

const fn editor_executable_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "Contents/MacOS"
    } else {
        "bin"
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
