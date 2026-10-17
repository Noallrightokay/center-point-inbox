// swift-tools-version:5.5
//
// RATA's StoreKit 2 plugin for iOS, linked into RATA for iOS by Tauri's
// plugin build (../build.rs, tauri-plugin's `ios_path`). It is only the
// Tauri glue: what talks to StoreKit is RATAStore (../store), which a later
// Mac App Store build can use as it is. StoreKit 2 needs iOS 15, which is
// RATA's own minimum (src-tauri/tauri.ios.conf.json); macOS 12 is named
// because RATAStore names it, and SwiftPM checks it whatever the target.
// "Tauri" is Tauri's Swift API, which that build copies into ../.tauri for
// the version of the tauri crate RATA is built with.

import PackageDescription

let package = Package(
  name: "tauri-plugin-storekit",
  platforms: [
    .iOS(.v15),
    .macOS(.v12),
  ],
  products: [
    .library(
      name: "tauri-plugin-storekit",
      type: .static,
      targets: ["tauri-plugin-storekit"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api"),
    .package(name: "RATAStore", path: "../store"),
  ],
  targets: [
    .target(
      name: "tauri-plugin-storekit",
      dependencies: [
        .byName(name: "Tauri"),
        .byName(name: "RATAStore"),
      ],
      path: "Sources")
  ]
)
