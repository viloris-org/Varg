#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Project build and package pipeline for Aster games.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use engine_assets::{AssetDatabase, scan_project_assets};
use engine_core::{EngineError, EngineResult};
use engine_ecs::{PROJECT_MANIFEST_FILE_NAME, project_manifest_path};
use runtime_min::load_runtime_project;
use serde::{Deserialize, Serialize};

/// Supported package targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageTarget {
    /// Native Linux x86_64 desktop target.
    LinuxX64,
    /// Native Windows x86_64 desktop target.
    WindowsX64,
    /// Native macOS universal desktop target.
    MacosUniversal,
    /// Android arm64 target.
    AndroidArm64,
    /// iOS universal target.
    IosUniversal,
}

impl PackageTarget {
    /// Parses a target alias used by editor and CLI surfaces.
    pub fn parse(input: &str) -> EngineResult<Self> {
        match input {
            "native" => Ok(Self::current_desktop()),
            "linux-x64" => Ok(Self::LinuxX64),
            "windows-x64" => Ok(Self::WindowsX64),
            "macos-universal" => Ok(Self::MacosUniversal),
            "android-arm64" => Ok(Self::AndroidArm64),
            "ios-universal" => Ok(Self::IosUniversal),
            other => Err(EngineError::config(format!(
                "unknown package target `{other}`"
            ))),
        }
    }

    /// Returns the canonical target alias.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux-x64",
            Self::WindowsX64 => "windows-x64",
            Self::MacosUniversal => "macos-universal",
            Self::AndroidArm64 => "android-arm64",
            Self::IosUniversal => "ios-universal",
        }
    }

    /// Returns the current host desktop target.
    pub const fn current_desktop() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::LinuxX64
        }
        #[cfg(target_os = "windows")]
        {
            Self::WindowsX64
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacosUniversal
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Self::LinuxX64
        }
    }

    /// Whether the target is a desktop runtime target.
    pub const fn is_desktop(self) -> bool {
        matches!(
            self,
            Self::LinuxX64 | Self::WindowsX64 | Self::MacosUniversal
        )
    }

    /// Whether the target is a mobile runtime target.
    pub const fn is_mobile(self) -> bool {
        matches!(self, Self::AndroidArm64 | Self::IosUniversal)
    }

    fn host_can_build(self) -> bool {
        match self {
            Self::LinuxX64 => cfg!(target_os = "linux"),
            Self::WindowsX64 => cfg!(target_os = "windows"),
            Self::MacosUniversal => cfg!(target_os = "macos"),
            Self::AndroidArm64 => cfg!(any(target_os = "linux", target_os = "windows")),
            Self::IosUniversal => cfg!(target_os = "macos"),
        }
    }
}

/// Supported package output formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageFormat {
    /// Runnable directory containing runtime, project files, manifests, and launcher.
    Folder,
    /// Android APK package.
    Apk,
    /// Android App Bundle package.
    Aab,
    /// iOS application archive.
    Ipa,
    /// Linux AppImage.
    AppImage,
    /// Debian package.
    Deb,
    /// RPM package.
    Rpm,
    /// Windows executable installer.
    Exe,
    /// Windows MSI installer.
    Msi,
    /// Windows NSIS installer.
    Nsis,
    /// macOS disk image.
    Dmg,
}

impl PackageFormat {
    /// Parses a format alias used by editor and CLI surfaces.
    pub fn parse(input: &str) -> EngineResult<Self> {
        match input {
            "folder" => Ok(Self::Folder),
            "apk" => Ok(Self::Apk),
            "aab" => Ok(Self::Aab),
            "ipa" => Ok(Self::Ipa),
            "appimage" => Ok(Self::AppImage),
            "deb" => Ok(Self::Deb),
            "rpm" => Ok(Self::Rpm),
            "exe" => Ok(Self::Exe),
            "msi" => Ok(Self::Msi),
            "nsis" => Ok(Self::Nsis),
            "dmg" => Ok(Self::Dmg),
            other => Err(EngineError::config(format!(
                "unknown package format `{other}`"
            ))),
        }
    }

    /// Returns the canonical format alias.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Apk => "apk",
            Self::Aab => "aab",
            Self::Ipa => "ipa",
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Exe => "exe",
            Self::Msi => "msi",
            Self::Nsis => "nsis",
            Self::Dmg => "dmg",
        }
    }
}

/// Package channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageChannel {
    /// Debug build.
    Debug,
    /// Release build.
    Release,
}

impl PackageChannel {
    /// Parses a channel alias.
    pub fn parse(input: &str) -> EngineResult<Self> {
        match input {
            "debug" => Ok(Self::Debug),
            "release" => Ok(Self::Release),
            other => Err(EngineError::config(format!(
                "channel must be `debug` or `release`, got `{other}`"
            ))),
        }
    }

    /// Returns the canonical channel alias.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    /// Whether this channel is optimized.
    pub const fn is_release(self) -> bool {
        matches!(self, Self::Release)
    }
}

/// Package request shared by CLI and editor surfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageRequest {
    /// Aster project root or manifest path.
    pub project: PathBuf,
    /// Repository root used for cargo builds.
    pub repo_root: PathBuf,
    /// Output target.
    pub target: PackageTarget,
    /// Output format.
    pub format: PackageFormat,
    /// Build channel.
    pub channel: PackageChannel,
    /// Whether asset optimization is requested.
    pub optimize_assets: bool,
    /// Whether debug symbols should be preserved in final output.
    pub include_debug_symbols: bool,
    /// Optional output directory override.
    pub output_dir: Option<PathBuf>,
}

/// Package output summary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PackageOutput {
    /// Project display name.
    pub project: String,
    /// Target alias.
    pub target: String,
    /// Format alias.
    pub format: String,
    /// Channel alias.
    pub channel: String,
    /// Package root directory.
    pub path: PathBuf,
    /// Runtime binary path, when one was produced.
    pub binary: Option<PathBuf>,
    /// Launcher path, when one was produced.
    pub launcher: Option<PathBuf>,
    /// Asset manifest path.
    pub assets_manifest: PathBuf,
    /// Number of manifest assets.
    pub asset_count: usize,
}

/// Builds and packages an Aster project.
pub fn package_project(request: &PackageRequest) -> EngineResult<PackageOutput> {
    validate_target_support(request.target, request.format)?;
    if request.target.is_mobile() {
        return Err(EngineError::UnsupportedCapability {
            capability: "mobile runtime adapter and signed package generation",
        });
    }

    let project = load_runtime_project(&request.project)?;
    let package_root = request.output_dir.clone().unwrap_or_else(|| {
        project
            .root
            .join("exports")
            .join(sanitize_package_path_segment(&project.manifest.name))
            .join(request.target.as_str())
            .join(request.channel.as_str())
    });
    let project_package_root = package_root.join("project");
    let bin_dir = package_root.join("bin");

    remove_dir_if_exists(&package_root)?;
    fs::create_dir_all(&project_package_root).map_err(|source| EngineError::Filesystem {
        path: project_package_root.clone(),
        source,
    })?;

    let assets_manifest = write_project_payload(
        &project.root,
        &project_package_root,
        &project.manifest.asset_root,
    )?;
    let asset_count = assets_manifest.entries.len();
    let assets_manifest_path = project_package_root
        .join(&project.manifest.asset_root)
        .join("asset-manifest.json");
    write_package_manifest(
        request,
        &package_root,
        &project.manifest.name,
        asset_count,
        &assets_manifest_path,
    )?;

    fs::create_dir_all(&bin_dir).map_err(|source| EngineError::Filesystem {
        path: bin_dir.clone(),
        source,
    })?;
    build_runtime_binary(
        &request.repo_root,
        request.channel.is_release(),
        &runtime_features(&project.build.features),
    )?;

    let runtime_source = request
        .repo_root
        .join("target")
        .join(if request.channel.is_release() {
            "release"
        } else {
            "debug"
        })
        .join(runtime_binary_file_name(request.target));
    let runtime_dest = bin_dir.join(packaged_runtime_file_name(request.target));
    fs::copy(&runtime_source, &runtime_dest).map_err(|source| EngineError::Filesystem {
        path: runtime_source.clone(),
        source,
    })?;
    let launcher = package_root.join(launcher_file_name(request.target));
    write_launcher(
        &package_root,
        request.target,
        packaged_runtime_file_name(request.target),
    )?;

    Ok(PackageOutput {
        project: project.manifest.name,
        target: request.target.as_str().to_owned(),
        format: request.format.as_str().to_owned(),
        channel: request.channel.as_str().to_owned(),
        path: package_root,
        binary: Some(runtime_dest),
        launcher: Some(launcher),
        assets_manifest: assets_manifest_path,
        asset_count,
    })
}

/// Validates host, format, and toolchain support for a target.
pub fn validate_target_support(target: PackageTarget, format: PackageFormat) -> EngineResult<()> {
    if !target.host_can_build() {
        return Err(EngineError::config(format!(
            "{} cannot be built from this host",
            target.as_str()
        )));
    }

    match target {
        PackageTarget::LinuxX64 if !matches!(format, PackageFormat::Folder) => {
            return Err(EngineError::UnsupportedCapability {
                capability: "linux installer package generation",
            });
        }
        PackageTarget::WindowsX64 if !matches!(format, PackageFormat::Folder) => {
            return Err(EngineError::UnsupportedCapability {
                capability: "windows installer package generation",
            });
        }
        PackageTarget::MacosUniversal if !matches!(format, PackageFormat::Folder) => {
            return Err(EngineError::UnsupportedCapability {
                capability: "macOS dmg/signing/notarization package generation",
            });
        }
        PackageTarget::AndroidArm64
            if !matches!(format, PackageFormat::Apk | PackageFormat::Aab) =>
        {
            return Err(EngineError::config(
                "android-arm64 supports apk or aab formats",
            ));
        }
        PackageTarget::IosUniversal if !matches!(format, PackageFormat::Ipa) => {
            return Err(EngineError::config("ios-universal supports ipa format"));
        }
        _ => {}
    }

    if target == PackageTarget::AndroidArm64 {
        require_env("ANDROID_HOME")?;
        require_env("ANDROID_NDK_HOME")?;
        require_rust_target("aarch64-linux-android")?;
    }
    if target == PackageTarget::IosUniversal {
        require_command("xcodebuild")?;
        require_rust_target("aarch64-apple-ios")?;
        require_rust_target("aarch64-apple-ios-sim")?;
    }
    Ok(())
}

fn write_project_payload(
    project_root: &Path,
    destination: &Path,
    asset_root: &str,
) -> EngineResult<engine_assets::ResourceManifestFormat> {
    copy_file(
        project_manifest_path(project_root),
        destination.join(PROJECT_MANIFEST_FILE_NAME),
    )?;
    let asset_source = project_root.join(asset_root);
    let asset_dest = destination.join(asset_root);
    copy_dir_filtered(&asset_source, &asset_dest, project_root)?;
    copy_dir_filtered(
        &project_root.join("scenes"),
        &destination.join("scenes"),
        project_root,
    )?;
    let build_config = project_root.join("build.runtime-min.toml");
    if build_config.is_file() {
        copy_file(build_config, destination.join("build.runtime-min.toml"))?;
    }
    let mut database = AssetDatabase::new(&asset_dest, "builtin");
    scan_project_assets(&asset_dest, &mut database)?;
    let manifest = database.manifest();
    let manifest_path = asset_dest.join("asset-manifest.json");
    let content = serde_json::to_string_pretty(&manifest).map_err(|error| {
        EngineError::other(format!("asset manifest serialization failed: {error}"))
    })?;
    fs::write(&manifest_path, content).map_err(|source| EngineError::Filesystem {
        path: manifest_path,
        source,
    })?;
    Ok(manifest)
}

fn build_runtime_binary(repo_root: &Path, release: bool, features: &str) -> EngineResult<()> {
    let mut command = Command::new("cargo");
    command
        .current_dir(repo_root)
        .arg("build")
        .arg("-p")
        .arg("runtime-min")
        .arg("--no-default-features")
        .arg("--features")
        .arg(features);
    if release {
        command.arg("--release");
    }
    let output = command.output().map_err(|source| EngineError::Filesystem {
        path: repo_root.join("cargo"),
        source,
    })?;
    if output.status.success() {
        return Ok(());
    }

    Err(EngineError::other(format!(
        "runtime build failed with status {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn runtime_features(configured: &[String]) -> String {
    let mut features = vec!["runtime-game".to_owned(), "wgpu".to_owned()];
    for feature in configured {
        if feature == "runtime-min" || feature == "runtime-game" {
            continue;
        }
        if !features.iter().any(|existing| existing == feature) {
            features.push(feature.clone());
        }
    }
    features.join(",")
}

fn copy_file(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> EngineResult<()> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|source_error| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source: source_error,
        })?;
    }
    fs::copy(source, destination).map_err(|source_error| EngineError::Filesystem {
        path: source.to_path_buf(),
        source: source_error,
    })?;
    Ok(())
}

fn copy_dir_filtered(source: &Path, destination: &Path, project_root: &Path) -> EngineResult<()> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(destination).map_err(|source_error| EngineError::Filesystem {
        path: destination.to_path_buf(),
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
        let source_path = entry.path();
        let file_name = entry.file_name();
        if source_path == project_root.join("exports") || file_name == "target" {
            continue;
        }
        let destination_path = destination.join(file_name);
        let file_type = entry
            .file_type()
            .map_err(|source_error| EngineError::Filesystem {
                path: source_path.clone(),
                source: source_error,
            })?;
        if file_type.is_dir() {
            copy_dir_filtered(&source_path, &destination_path, project_root)?;
        } else if file_type.is_file() {
            copy_file(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn write_launcher(
    package_root: &Path,
    target: PackageTarget,
    runtime_name: &str,
) -> EngineResult<()> {
    let launcher_path = package_root.join(launcher_file_name(target));
    #[cfg(target_os = "windows")]
    let launcher = format!("@echo off\r\n\"%~dp0bin\\{runtime_name}\" \"%~dp0project\"\r\n");
    #[cfg(not(target_os = "windows"))]
    let launcher = format!(
        "#!/usr/bin/env sh\nDIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$DIR/bin/{runtime_name}\" \"$DIR/project\"\n"
    );
    fs::write(&launcher_path, launcher).map_err(|source| EngineError::Filesystem {
        path: launcher_path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&launcher_path)
            .map_err(|source| EngineError::Filesystem {
                path: launcher_path.clone(),
                source,
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&launcher_path, permissions).map_err(|source| {
            EngineError::Filesystem {
                path: launcher_path,
                source,
            }
        })?;
    }
    Ok(())
}

fn write_package_manifest(
    request: &PackageRequest,
    package_root: &Path,
    project_name: &str,
    asset_count: usize,
    assets_manifest_path: &Path,
) -> EngineResult<()> {
    let manifest_path = package_root.join("package-manifest.json");
    let relative_assets_manifest = assets_manifest_path
        .strip_prefix(package_root)
        .unwrap_or(assets_manifest_path);
    let manifest = serde_json::json!({
        "schema": "aster.package.v1",
        "project": project_name,
        "target": request.target.as_str(),
        "format": request.format.as_str(),
        "channel": request.channel.as_str(),
        "runtime": format!("bin/{}", packaged_runtime_file_name(request.target)),
        "project_root": "project",
        "launcher": launcher_file_name(request.target),
        "assets_manifest": relative_assets_manifest,
        "asset_count": asset_count,
        "optimize_assets": request.optimize_assets,
        "include_debug_symbols": request.include_debug_symbols,
        "created_at": timestamp_now(),
    });
    let content = serde_json::to_string_pretty(&manifest).map_err(|error| {
        EngineError::other(format!("package manifest serialization failed: {error}"))
    })?;
    fs::write(&manifest_path, content).map_err(|source| EngineError::Filesystem {
        path: manifest_path,
        source,
    })
}

fn require_env(name: &str) -> EngineResult<()> {
    match env::var_os(name) {
        Some(value) if !value.is_empty() => Ok(()),
        _ => Err(EngineError::config(format!(
            "{name} must be set to build this target"
        ))),
    }
}

fn require_command(name: &str) -> EngineResult<()> {
    let status = Command::new(name).arg("-version").output();
    match status {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) | Err(_) => Err(EngineError::config(format!(
            "`{name}` must be available on PATH"
        ))),
    }
}

fn require_rust_target(target: &str) -> EngineResult<()> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|source| EngineError::Filesystem {
            path: PathBuf::from("rustup"),
            source,
        })?;
    if !output.status.success() {
        return Err(EngineError::other(format!(
            "rustup target list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == target) {
        Ok(())
    } else {
        Err(EngineError::config(format!(
            "Rust target `{target}` is not installed; run `rustup target add {target}`"
        )))
    }
}

fn remove_dir_if_exists(path: &Path) -> EngineResult<()> {
    if !path.exists() {
        return Ok(());
    }
    fs::remove_dir_all(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

fn sanitize_package_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    if out.is_empty() {
        "project".to_owned()
    } else {
        out
    }
}

fn runtime_binary_file_name(target: PackageTarget) -> &'static str {
    if target == PackageTarget::WindowsX64 {
        "runtime-min.exe"
    } else {
        "runtime-min"
    }
}

fn packaged_runtime_file_name(target: PackageTarget) -> &'static str {
    if target == PackageTarget::WindowsX64 {
        "aster-runtime.exe"
    } else {
        "aster-runtime"
    }
}

fn launcher_file_name(target: PackageTarget) -> &'static str {
    if target == PackageTarget::WindowsX64 {
        "run.bat"
    } else {
        "run.sh"
    }
}

fn timestamp_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_aliases() {
        assert_eq!(
            PackageTarget::parse("android-arm64").unwrap(),
            PackageTarget::AndroidArm64
        );
        assert_eq!(
            PackageFormat::parse("folder").unwrap(),
            PackageFormat::Folder
        );
        assert_eq!(
            PackageChannel::parse("release").unwrap(),
            PackageChannel::Release
        );
    }

    #[test]
    fn rejects_mobile_format_mismatch() {
        let error = validate_target_support(PackageTarget::AndroidArm64, PackageFormat::Folder)
            .unwrap_err();
        assert!(error.to_string().contains("android-arm64 supports apk"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn android_requires_toolchain_before_mobile_adapter() {
        let error =
            validate_target_support(PackageTarget::AndroidArm64, PackageFormat::Apk).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("ANDROID_") || message.contains("Rust target"));
    }
}
