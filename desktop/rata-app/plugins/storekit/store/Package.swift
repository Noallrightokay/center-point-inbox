// swift-tools-version:5.5
//
// RATAStore: RATA's purchases through StoreKit 2, and nothing else. It
// imports no Tauri, UIKit or AppKit, so the same code serves the iPhone
// app's Tauri plugin (../ios, which depends on this package) and a later
// Mac App Store build: StoreKit 2 is one API on iOS 15 and macOS 12. What
// needs a window, the purchase sheet over RATA's own window and Apple's
// subscriptions sheet, is the wrapper's.
//
// ../testing runs it against ios/RATA.storekit, in a host app.

import PackageDescription

let package = Package(
  name: "RATAStore",
  platforms: [
    .iOS(.v15),
    .macOS(.v12),
  ],
  products: [
    .library(
      name: "RATAStore",
      type: .static,
      targets: ["RATAStore"])
  ],
  targets: [
    .target(
      name: "RATAStore",
      path: "Sources/RATAStore")
  ]
)
