#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use engine_core::{EngineError, EngineResult};
use engine_ecs::{BuildConfiguration, ProjectManifest};

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
        Some("package") => {
            let request = parse_package_request(args)?;
            package(request)
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

#[derive(Clone, Debug)]
struct PackageRequest {
    profile: Profile,
    project: PathBuf,
    release: bool,
}

fn parse_package_request(args: impl Iterator<Item = String>) -> EngineResult<PackageRequest> {
    let mut profile = None;
    let mut project = None;
    let mut release = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => {
                let value = args
                    .next()
                    .ok_or_else(|| EngineError::config("expected value after --profile"))?;
                profile = Some(Profile::parse(&value)?);
            }
            "--project" => {
                let value = args
                    .next()
                    .ok_or_else(|| EngineError::config("expected value after --project"))?;
                project = Some(PathBuf::from(value));
            }
            "--release" => release = true,
            value if value.starts_with("--") => {
                return Err(EngineError::config(format!(
                    "unsupported package flag `{value}`"
                )));
            }
            value => {
                if profile.is_none() && matches!(value, "runtime-game" | "editor") {
                    profile = Some(Profile::parse(value)?);
                } else if project.is_none() {
                    project = Some(PathBuf::from(value));
                } else {
                    return Err(EngineError::config(format!(
                        "unexpected package argument `{value}`"
                    )));
                }
            }
        }
    }

    let project = project.unwrap_or_else(|| PathBuf::from("examples/project"));
    let profile = match profile {
        Some(profile) => profile,
        None => profile_from_project_config(&project)?,
    };

    Ok(PackageRequest {
        profile,
        project,
        release,
    })
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
    cargo_vec(&base_args)?;

    match profile {
        Profile::Editor | Profile::AgentTools | Profile::DevFull => {
            let mut aster_args = vec![
                "build",
                "-p",
                "aster",
                "--no-default-features",
                "--features",
                profile.name(),
            ];
            if release {
                aster_args.push("--release");
            }
            cargo_vec(&aster_args)
        }
        _ => Ok(()),
    }
}

fn package(request: PackageRequest) -> EngineResult<()> {
    let profile = request.profile;
    match profile {
        Profile::RuntimeGame | Profile::Editor => {}
        unsupported => {
            return Err(EngineError::config(format!(
                "native packaging is supported for runtime-game and editor, got `{}`",
                unsupported.name()
            )));
        }
    }

    build_profile(profile, request.release)?;
    let mut aster_args = vec![
        "build",
        "-p",
        "aster",
        "--no-default-features",
        "--features",
        profile.name(),
    ];
    if request.release {
        aster_args.push("--release");
    }
    cargo_vec(&aster_args)?;

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

    let build_profile_dir = if request.release { "release" } else { "debug" };
    let cli_source = PathBuf::from("target")
        .join(build_profile_dir)
        .join(executable_name("aster"));
    let cli_dest = bin_dir.join(executable_name("aster"));
    fs::copy(&cli_source, &cli_dest).map_err(|source| EngineError::Filesystem {
        path: cli_source.clone(),
        source,
    })?;

    if profile == Profile::Editor {
        create_editor_app_layout(&package_root, &cli_dest)?;
    }

    copy_project_payload(&request.project, &package_root)?;
    write_manifest(&package_root, profile)?;
    write_marker_artifact(&package_root, profile)?;
    println!(
        "packaged {} project {} at {}",
        profile.name(),
        request.project.display(),
        package_root.display()
    );
    Ok(())
}

fn profile_from_project_config(project_root: &Path) -> EngineResult<Profile> {
    let config_path = project_root.join("build.runtime-min.toml");
    let text = fs::read_to_string(&config_path).map_err(|source| EngineError::Filesystem {
        path: config_path.clone(),
        source,
    })?;
    let config = toml::from_str::<BuildConfiguration>(&text)
        .map_err(|error| EngineError::config(format!("build config parse failed: {error}")))?;
    if config
        .features
        .iter()
        .any(|feature| feature == "runtime-game")
    {
        Ok(Profile::RuntimeGame)
    } else if config.features.iter().any(|feature| feature == "editor") {
        Ok(Profile::Editor)
    } else {
        Ok(Profile::RuntimeGame)
    }
}

fn copy_project_payload(project_root: &Path, package_root: &Path) -> EngineResult<()> {
    let manifest_path = project_root.join("aster.project.toml");
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(|source| EngineError::Filesystem {
            path: manifest_path.clone(),
            source,
        })?;
    let manifest = toml::from_str::<ProjectManifest>(&manifest_text)
        .map_err(|error| EngineError::config(format!("project manifest parse failed: {error}")))?;
    if let Some(diagnostic) = manifest.diagnostics().into_iter().next() {
        return Err(EngineError::config(format!(
            "{}: {}",
            diagnostic.path, diagnostic.message
        )));
    }

    let project_dest = package_root.join("project");
    recreate_dir(&project_dest)?;
    copy_file(&manifest_path, &project_dest.join("aster.project.toml"))?;
    copy_file(
        &project_root.join("build.runtime-min.toml"),
        &project_dest.join("build.runtime-min.toml"),
    )?;
    copy_file(
        &project_root.join(&manifest.default_scene),
        &project_dest.join(&manifest.default_scene),
    )?;
    copy_dir_if_exists(
        &project_root.join(&manifest.asset_root),
        &project_dest.join(&manifest.asset_root),
    )?;
    fs::create_dir_all(project_dest.join("import-cache")).map_err(|source| {
        EngineError::Filesystem {
            path: project_dest.join("import-cache"),
            source,
        }
    })?;
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

fn copy_file(source: &Path, dest: &Path) -> EngineResult<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::copy(source, dest)
        .map(|_| ())
        .map_err(|source_error| EngineError::Filesystem {
            path: source.to_path_buf(),
            source: source_error,
        })
}

fn copy_dir_if_exists(source: &Path, dest: &Path) -> EngineResult<()> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest).map_err(|source_error| EngineError::Filesystem {
        path: dest.to_path_buf(),
        source: source_error,
    })?;
    for entry in fs::read_dir(source).map_err(|source_error| EngineError::Filesystem {
        path: source.to_path_buf(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| EngineError::Filesystem {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source_error| EngineError::Filesystem {
                path: entry.path(),
                source: source_error,
            })?;
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_if_exists(&entry.path(), &dest_path)?;
        } else if file_type.is_file() {
            copy_file(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
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
