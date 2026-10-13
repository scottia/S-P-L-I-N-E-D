use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=gui/app.ico");
    println!("cargo:rerun-if-changed=gui/app.manifest");
    println!("cargo:rerun-if-env-changed=SPLINED_BUILD_COMMIT");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));
    let commit = build_commit(&manifest_dir);
    println!("cargo:rustc-env=SPLINED_BUILD_COMMIT={commit}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let manifest = manifest_dir.join("gui/app.manifest");
        let icon = manifest_dir.join("gui/app.ico");
        let resource = compile_windows_resources(&manifest_dir, &icon);
        println!("cargo:rustc-link-arg-bin=splined=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=splined=/MANIFESTINPUT:{}",
            manifest.display()
        );
        println!("cargo:rustc-link-arg-bin=splined={}", resource.display());
    }

    slint_build::compile("ui/app.slint").expect("unable to compile the SPLINED Slint UI");
}

fn compile_windows_resources(manifest_dir: &std::path::Path, icon: &std::path::Path) -> PathBuf {
    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    let script = output_dir.join("splined-icon.rc");
    let resource = output_dir.join("splined-icon.res");
    let icon_path = icon.display().to_string().replace(['\\', '/'], "\\\\");
    fs::write(&script, format!("1 ICON \"{icon_path}\"\r\n"))
        .expect("unable to write the SPLINED Windows resource script");
    let compiler = find_resource_compiler(manifest_dir)
        .expect("Windows SDK rc.exe is required to embed the SPLINED executable icon");
    let status = Command::new(&compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&script)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "unable to run Windows resource compiler {}: {error}",
                compiler.display()
            )
        });
    assert!(
        status.success() && resource.is_file(),
        "Windows resource compiler failed while embedding gui/app.ico"
    );
    resource
}

fn find_resource_compiler(manifest_dir: &std::path::Path) -> Option<PathBuf> {
    if let Some(configured) = env::var_os("RC").map(PathBuf::from)
        && configured.is_file()
    {
        return Some(configured);
    }
    if let Ok(output) = Command::new("where.exe").arg("rc.exe").output()
        && output.status.success()
        && let Ok(text) = String::from_utf8(output.stdout)
        && let Some(path) = text.lines().map(str::trim).find(|line| !line.is_empty())
    {
        return Some(PathBuf::from(path));
    }

    let kits = env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
        .join("Windows Kits/10/bin");
    let architecture = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "x64",
    };
    let mut candidates = fs::read_dir(kits)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(architecture).join("rc.exe"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop().or_else(|| {
        // Keeps the diagnostic rooted at the Cargo project when a custom SDK
        // wrapper is checked into a controlled build environment.
        let local = manifest_dir.join("tools/rc.exe");
        local.is_file().then_some(local)
    })
}

fn build_commit(manifest_dir: &std::path::Path) -> String {
    env::var("SPLINED_BUILD_COMMIT")
        .ok()
        .filter(|value| valid_commit(value))
        .or_else(|| git_commit(manifest_dir))
        .unwrap_or_else(|| "unknown".to_string())
}

fn valid_commit(value: &str) -> bool {
    let value = value.trim();
    (7..=40).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_commit(manifest_dir: &std::path::Path) -> Option<String> {
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
