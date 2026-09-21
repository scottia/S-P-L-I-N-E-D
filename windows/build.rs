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
    "EmbeddedAssets.cs",
    "ConfigState.cs",
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
    println!("cargo:rerun-if-changed=gui/app.rc");
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

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    let embedded_gui = out_dir.join("splined-gui.exe");
    compile_gui(&gui_dir, &embedded_gui);
    compile_native_resources(&gui_dir, &out_dir);

    println!(
        "cargo:rustc-env=SPLINED_EMBEDDED_GUI={}",
        embedded_gui.display()
    );
}

fn compile_gui(gui_dir: &Path, output_path: &Path) {
    let csc = locate_csc()
        .unwrap_or_else(|| panic!("Microsoft .NET Framework C# compiler csc.exe was not found"));
    let manifest = gui_dir.join("app.manifest");
    let icon = gui_dir.join("app.ico");
    let mut arguments = vec![
        OsString::from("/nologo"),
        OsString::from("/target:winexe"),
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

    run_checked(
        Command::new(&csc).current_dir(gui_dir).args(&arguments),
        "SPLINED embedded Windows GUI compilation failed",
    );
}

fn compile_native_resources(gui_dir: &Path, out_dir: &Path) {
    let rc = locate_resource_compiler()
        .unwrap_or_else(|| panic!("Windows SDK resource compiler rc.exe was not found"));
    let resource = out_dir.join("splined.res");
    let mut output_argument = OsString::from("/fo");
    output_argument.push(resource.as_os_str());
    run_checked(
        Command::new(&rc)
            .current_dir(gui_dir)
            .arg("/nologo")
            .arg(output_argument)
            .arg("app.rc"),
        "SPLINED native icon/version resource compilation failed",
    );
    println!("cargo:rustc-link-arg-bin=splined={}", resource.display());
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

fn locate_resource_compiler() -> Option<PathBuf> {
    let program_files = env::var_os("ProgramFiles(x86)")?;
    let bin_root = PathBuf::from(program_files).join("Windows Kits/10/bin");
    let mut versions = fs::read_dir(bin_root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .collect::<Vec<_>>();
    versions.sort_by_key(|entry| entry.file_name());
    versions.reverse();
    versions
        .into_iter()
        .map(|entry| entry.path().join("x64/rc.exe"))
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
