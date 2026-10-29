use std::env;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

// Commands the webview may reach through the ACL; `permissions/default.toml` decides which
// ones the default set grants. Rust callers go through `KeychainExt` and need no permission.
const COMMANDS: &[&str] = &[
    "store",
    "retrieve",
    "remove",
    "set_privacy_screen",
    "set_status_bar_style",
    "status",
    "exclude_from_backup",
];

/// The Touch ID backend's Swift package (`macos/`, see `src/desktop/macos.rs`).
#[cfg(target_os = "macos")]
const MACOS_PACKAGE: &str = "MoonyBiometrics";
/// CryptoKit's first macOS.
#[cfg(target_os = "macos")]
const MACOS_MIN_VERSION: &str = "10.15";

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();

    // Checked on the target, not with `#[cfg]`: a build script is compiled for the host, and the
    // iOS build (also on a Mac host) links the ../ios package through tauri-plugin instead.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        link_macos_package();
    }
}

#[cfg(not(target_os = "macos"))]
fn link_macos_package() {
    panic!("the macOS target builds only on a Mac host (swift-rs and the Swift toolchain)");
}

#[cfg(target_os = "macos")]
fn link_macos_package() {
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("cargo sets CARGO_CFG_TARGET_ARCH");
    if target_arch == env::consts::ARCH {
        swift_rs::SwiftLinker::new(MACOS_MIN_VERSION)
            .with_package(MACOS_PACKAGE, "macos/")
            .link();
    } else {
        // swift-rs builds packages for the build script's own architecture (the host's), so the
        // Intel release leg, cross-built on an Apple Silicon runner, would get arm64 objects and
        // fail to link. swift-rs still links the Swift runtime (it reads the target from cargo);
        // the package is built here for the target's architecture.
        swift_rs::SwiftLinker::new(MACOS_MIN_VERSION).link();
        build_macos_package_for(&target_arch);
    }
}

/// `swift build` the package for `rust_arch` the way swift-rs does for the host, then link it.
#[cfg(target_os = "macos")]
fn build_macos_package_for(rust_arch: &str) {
    let arch = if rust_arch == "aarch64" {
        "arm64"
    } else {
        rust_arch
    };
    let triple = format!("{arch}-apple-macosx{MACOS_MIN_VERSION}");
    let configuration = if env::var("DEBUG").as_deref() == Ok("true") {
        "debug"
    } else {
        "release"
    };
    let package_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join("macos");
    let build_path = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("swift-rs")
        .join(MACOS_PACKAGE);

    let sdk_output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("xcrun runs");
    assert!(sdk_output.status.success(), "xcrun found no macOS SDK");
    let sdk = String::from_utf8_lossy(&sdk_output.stdout)
        .trim()
        .to_string();

    let status = Command::new("swift")
        .current_dir(&package_dir)
        .args(["build", "--sdk", &sdk, "-c", configuration, "--arch", arch])
        .arg("--build-path")
        .arg(&build_path)
        .args(["-Xswiftc", "-sdk", "-Xswiftc", &sdk])
        .args(["-Xswiftc", "-target", "-Xswiftc", &triple])
        .args(["-Xcc", &format!("--target={triple}")])
        .status()
        .expect("swift runs");
    assert!(
        status.success(),
        "swift build of {MACOS_PACKAGE} for {arch} failed"
    );

    // SwiftPM's older layout keeps the products under `<arch>-apple-macosx/<configuration>`;
    // Xcode 27's under `out/Products/…`, with `<configuration>` linked to them.
    let library = format!("lib{MACOS_PACKAGE}.a");
    let products = [
        build_path
            .join(format!("{arch}-apple-macosx"))
            .join(configuration),
        build_path.join(configuration),
    ]
    .into_iter()
    .find(|dir| dir.join(&library).exists())
    .expect("swift build left the static library where SwiftPM puts it");

    println!("cargo:rerun-if-changed={}", package_dir.display());
    println!("cargo:rustc-link-search=native={}", products.display());
    println!("cargo:rustc-link-lib=static={MACOS_PACKAGE}");
}
