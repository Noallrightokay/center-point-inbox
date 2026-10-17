// swift-tools-version:5.5
//
// RATA's StoreKit 2 plugin, linked into RATA for iOS by Tauri's plugin build
// (../build.rs, tauri-plugin's `ios_path`). StoreKit 2 needs iOS 15, which
// is RATA's own minimum (src-tauri/tauri.ios.conf.json). "Tauri" is Tauri's
// Swift API, which that build copies into ../.tauri for the version of the
// tauri crate RATA is built with.

import PackageDescription

let package = Package(
  name: "tauri-plugin-storekit",
  platforms: [
    .iOS(.v15)
  ],
  products: [
    .library(
      name: "tauri-plugin-storekit",
      type: .static,
      targets: ["tauri-plugin-storekit"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-storekit",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
