// Commands the webview may reach through the ACL; `permissions/default.toml` decides which
// ones the default set grants. Rust callers go through `KeychainExt` and need no permission.
const COMMANDS: &[&str] = &[
    "store",
    "retrieve",
    "remove",
    "set_privacy_screen",
    "status",
    "exclude_from_backup",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
