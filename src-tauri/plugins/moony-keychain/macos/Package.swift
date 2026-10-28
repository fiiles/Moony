// swift-tools-version:5.5
// The macOS half of the moony-keychain plugin: Touch ID unlock with a Secure Enclave key.
// The plugin's build script links this package into the Rust library with swift-rs, on macOS only
// (`build.rs`); the iOS half is the separate package in ../ios. SwiftRs is the Swift side of the
// swift-rs types (`SRData`, `SRString`) the exported functions take and return; it is pinned to the
// exact release the Rust crate uses (Cargo.lock), and `Package.resolved` is committed with it, so a
// new upstream tag cannot change what the release build links. 10.15 is the first macOS with
// CryptoKit.

import PackageDescription

let package = Package(
  name: "MoonyBiometrics",
  platforms: [
    .macOS(.v10_15)
  ],
  products: [
    .library(
      name: "MoonyBiometrics",
      type: .static,
      targets: ["MoonyBiometrics"])
  ],
  dependencies: [
    .package(name: "SwiftRs", url: "https://github.com/Brendonovich/swift-rs", .exact("1.0.9"))
  ],
  targets: [
    .target(
      name: "MoonyBiometrics",
      dependencies: [
        .product(name: "SwiftRs", package: "SwiftRs")
      ],
      path: "Sources/MoonyBiometrics")
  ]
)
