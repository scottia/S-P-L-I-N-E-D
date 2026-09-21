use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GUI_SOURCES: &[&str] = &[
    "ReleaseInfo.cs",
    "Program.cs",
    "AssemblyInfo.cs",
    "AppIcon.cs",
    "ConfigState.cs",
    "ThemeManager.cs",
    "SetupForm.cs",
    "SupportWindows.cs",
    "LibraryModel.cs",
    "MainForm.cs",
];

const RUNTIME_ASSETS: &[&str] = &["app.ico", "splined-app-icon.png", "splined-watermark.png"];

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));
    let gui_dir = manifest_dir.join("gui");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=gui/app.manifest");
    for source in GUI_SOURCES {
        println!("cargo:rerun-if-changed=gui/{source}");
    }
    for asset in RUNTIME_ASSETS {
        println!("cargo:rerun-if-changed=gui/{asset}");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!("cargo:warning=The SPLINED Windows GUI is compiled only for a Windows target.");
        return;
    }

    let csc = locate_csc()
        .unwrap_or_else(|| panic!("Microsoft .NET Framework C# compiler csc.exe was not found"));
    let profile_dir = cargo_profile_dir();
    fs::create_dir_all(&profile_dir).expect("unable to create Cargo profile directory");

    let executable = profile_dir.join("splined.exe");
    let manifest = gui_dir.join("app.manifest");
    let icon = gui_dir.join("app.ico");

    let mut arguments = vec![
        OsString::from("/nologo"),
        OsString::from("/target:winexe"),
        OsString::from("/main:Splined.WindowsGui.Program"),
        OsString::from("/optimize+"),
        option("/win32manifest:", &manifest),
        option("/win32icon:", &icon),
        option("/out:", &executable),
        OsString::from("/reference:System.dll"),
        OsString::from("/reference:System.Core.dll"),
        OsString::from("/reference:System.Drawing.dll"),
        OsString::from("/reference:System.Windows.Forms.dll"),
        OsString::from("/reference:System.Web.Extensions.dll"),
    ];
    arguments.extend(GUI_SOURCES.iter().map(OsString::from));

    let output = Command::new(&csc)
        .current_dir(&gui_dir)
        .args(&arguments)
        .output()
        .expect("unable to start the .NET Framework C# compiler");
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        panic!("SPLINED Windows GUI compilation failed");
    }

    for asset in RUNTIME_ASSETS {
        fs::copy(gui_dir.join(asset), profile_dir.join(asset))
            .unwrap_or_else(|error| panic!("unable to stage {asset}: {error}"));
    }
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

fn cargo_profile_dir() -> PathBuf {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    out_dir
        .ancestors()
        .nth(3)
        .expect("unexpected Cargo OUT_DIR layout")
        .to_path_buf()
}

fn option(prefix: &str, path: &Path) -> OsString {
    let mut value = OsString::from(prefix);
    value.push(path.as_os_str());
    value
}
