fn main() {
    link_openssl_system_libraries();
    tauri_build::build()
}

/// On Windows SQLCipher links OpenSSL's libcrypto statically (`release.yml` installs vcpkg's
/// `x64-windows-static-md` with `OPENSSL_STATIC=1`), and a static libcrypto needs these system
/// libraries. libsqlite3-sys names only libcrypto itself. Until 1.3.0 the umbrella import library
/// of windows-targets 0.53 supplied them by accident; when the 1.3.0 dependency update dropped that
/// crate, the release build failed to link `moony_lib.dll` (`GetProcessWindowStation` and
/// `GetUserObjectInformationW` from user32, called by `OPENSSL_isservice`).
///
/// The target comes from `CARGO_CFG_TARGET_OS`: a `cfg!` here would describe the build host.
fn link_openssl_system_libraries() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for lib in ["user32", "advapi32", "crypt32", "ws2_32", "gdi32"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
}
