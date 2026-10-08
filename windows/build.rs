use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GUI_SOURCES: &[&str] = &[
    "ReleaseInfo.cs",
    "NativeCore.cs",
    "Program.cs",
    "AssemblyInfo.cs",
    "AppIcon.cs",
    "EmbeddedAssets.cs",
    "ConfigState.cs",
    "BackupWindows.cs",
    "MusicBrainzMatchesForm.cs",
    "RuntimeLog.cs",
    "UpdateService.cs",
    "ThemeManager.cs",
    "SetupForm.cs",
    "SupportWindows.cs",
    "LibraryModel.cs",
    "MainForm.cs",
];

const GUI_RESOURCES: &[(&str, &str)] = &[
    (
        "splined-watermark.png",
        "Splined.WindowsGui.Resources.splined-watermark.png",
    ),
    (
        "splined-app-icon.png",
        "Splined.WindowsGui.Resources.splined-app-icon.png",
    ),
];

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));
    let gui_dir = manifest_dir.join("gui");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=gui/app.manifest");
    println!("cargo:rerun-if-changed=gui/app.ico");
    println!("cargo:rerun-if-env-changed=SPLINED_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=SPLINED_BUILD_GUI");
    for source in GUI_SOURCES {
        println!("cargo:rerun-if-changed=gui/{source}");
    }
    for (asset, _) in GUI_RESOURCES {
        println!("cargo:rerun-if-changed=gui/{asset}");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!("cargo:warning=The SPLINED Windows GUI is compiled only for a Windows target.");
        return;
    }

    // Cargo checks and Rust tests must not manufacture additional GUI executables.
    // The release/packaging entry point opts in once after the Rust DLL is built.
    if env::var("SPLINED_BUILD_GUI").as_deref() != Ok("1") {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("Unable to determine the Cargo profile output directory"));
    let gui_executable = profile_dir.join("splined.exe");
    let commit = build_commit(&manifest_dir);
    println!("cargo:rustc-env=SPLINED_BUILD_COMMIT={commit}");
    let build_info = write_build_info(&out_dir, &commit);
    compile_gui(&gui_dir, &gui_executable, &build_info);
}

fn compile_gui(gui_dir: &Path, output_path: &Path, build_info: &Path) {
    let csc = locate_csc()
        .unwrap_or_else(|| panic!("Microsoft .NET Framework C# compiler csc.exe was not found"));
    let manifest = gui_dir.join("app.manifest");
    let icon = gui_dir.join("app.ico");
    let mut arguments = vec![
        OsString::from("/nologo"),
        OsString::from("/target:winexe"),
        OsString::from("/platform:x64"),
        OsString::from("/main:Splined.WindowsGui.Program"),
        OsString::from("/optimize+"),
        option("/win32manifest:", &manifest),
        option("/win32icon:", &icon),
        option("/out:", output_path),
        OsString::from("/reference:System.dll"),
        OsString::from("/reference:System.Core.dll"),
        OsString::from("/reference:System.Drawing.dll"),
        OsString::from("/reference:System.Windows.Forms.dll"),
        OsString::from("/reference:System.Web.Extensions.dll"),
    ];
    for (asset, name) in GUI_RESOURCES {
        arguments.push(resource_option(&gui_dir.join(asset), name));
    }
    arguments.extend(GUI_SOURCES.iter().map(OsString::from));
    arguments.push(build_info.as_os_str().to_os_string());

    run_checked(
        Command::new(&csc).current_dir(gui_dir).args(&arguments),
        "SPLINED Windows GUI compilation failed",
    );
}

fn build_commit(manifest_dir: &Path) -> String {
    env::var("SPLINED_BUILD_COMMIT")
        .ok()
        .filter(|value| valid_commit(value))
        .or_else(|| git_commit(manifest_dir))
        .unwrap_or_else(|| "unknown".to_string())
}

fn write_build_info(out_dir: &Path, commit: &str) -> PathBuf {
    let short_commit = if commit == "unknown" {
        commit.to_string()
    } else {
        commit.chars().take(7).collect()
    };
    let source = format!(
        "namespace Splined.WindowsGui\n{{\n    internal static class BuildInfo\n    {{\n        public const string Commit = \"{commit}\";\n        public const string ShortCommit = \"{short_commit}\";\n    }}\n}}\n"
    );
    let path = out_dir.join("BuildInfo.cs");
    fs::write(&path, source).unwrap_or_else(|error| {
        panic!(
            "Unable to write Windows GUI build metadata {}: {error}",
            path.display()
        )
    });
    path
}

fn valid_commit(value: &str) -> bool {
    let value = value.trim();
    (7..=40).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_commit(manifest_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    valid_commit(value).then(|| value.to_ascii_lowercase())
}

fn locate_csc() -> Option<PathBuf> {
    let windows = PathBuf::from(env::var_os("WINDIR")?);
    [
        windows.join("Microsoft.NET/Framework64/v4.0.30319/csc.exe"),
        windows.join("Microsoft.NET/Framework/v4.0.30319/csc.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

fn run_checked(command: &mut Command, failure: &str) {
    let output = command
        .output()
        .expect("unable to start Windows build tool");
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        panic!("{failure}");
    }
}

fn option(prefix: &str, path: &Path) -> OsString {
    let mut value = OsString::from(prefix);
    value.push(path.as_os_str());
    value
}

fn resource_option(path: &Path, name: &str) -> OsString {
    let mut value = option("/resource:", path);
    value.push(",");
    value.push(name);
    value
}
