// swift-tools-version:5.3
// The iOS half of the moony-keychain plugin. tauri-plugin's build script copies the Tauri iOS
// API into ../.tauri/tauri-api and links this package into the app's Rust library.

import PackageDescription

let package = Package(
  name: "moony-keychain",
  platforms: [
    .macOS(.v10_13),
    .iOS(.v13),
  ],
  products: [
    .library(
      name: "moony-keychain",
      type: .static,
      targets: ["moony-keychain"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "moony-keychain",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
