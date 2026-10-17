// RATA's In-App Purchase on iOS: the Tauri plugin around RATAStore.
//
// RATAStore (../../store) is what talks to StoreKit 2, and decides nothing
// for RATA; RATA's Rust (src-tauri/src/appstore.rs) decides the rest. This
// file only hands each command from Rust to RATAStore and its answer back,
// and does the two things that need RATA's window on an iPhone: the
// purchase sheet over it, and Apple's subscriptions sheet. The page cannot
// reach any of it: no permission of Tauri's allows a command of this plugin.
//
// Every command answers from a task of its own: Tauri hands commands over on
// one queue, and a purchase can wait on the customer for minutes.

import Foundation
import RATAStore
import StoreKit
import SwiftRs
import Tauri
import UIKit
import WebKit

struct ProductsArgs: Decodable {
  let ids: [String]
}

struct IdArgs: Decodable {
  let id: String
}

struct ListenArgs: Decodable {
  let handler: Channel
}

@available(iOS 15.0, *)
class StoreKitPlugin: Plugin {
  private let store = Store()

  /// `Transaction.updates` is listened to from the moment the plugin loads,
  /// as Apple asks; what it brings waits in RATAStore until Rust listens.
  @objc public override func load(webview: WKWebView) {
    store.start()
  }

  /// Rust's channel for `Transaction.updates`, and what came before it.
  @objc public func listen(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ListenArgs.self)
    let channel = args.handler
    store.listen { signed in
      try? channel.send(signed)
    }
    invoke.resolve()
  }

  @objc public func products(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ProductsArgs.self)
    Task {
      do {
        invoke.resolve(try await self.store.products(args.ids))
      } catch {
        invoke.reject(error.localizedDescription)
      }
    }
  }

  @objc public func purchase(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(IdArgs.self)
    Task { @MainActor in
      invoke.resolve(await self.store.purchase(args.id) { try await self.buy($0) })
    }
  }

  /// The purchase sheet, over RATA's own window where iOS lets it be named.
  @MainActor
  private func buy(_ product: Product) async throws -> Product.PurchaseResult {
    if #available(iOS 17.0, *), let scene = self.scene() {
      return try await product.purchase(confirmIn: scene)
    }
    return try await product.purchase()
  }

  @MainActor
  private func scene() -> UIWindowScene? {
    if let scene = self.manager.viewController?.view.window?.windowScene {
      return scene
    }
    return UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .first { $0.activationState == .foregroundActive }
  }

  @objc public func entitlements(_ invoke: Invoke) {
    Task {
      invoke.resolve(await self.store.entitlements())
    }
  }

  @objc public func restore(_ invoke: Invoke) {
    Task {
      do {
        invoke.resolve(try await self.store.restore())
      } catch {
        invoke.reject(error.localizedDescription)
      }
    }
  }

  /// Apple's own sheet for managing or cancelling the subscription.
  @objc public func manage(_ invoke: Invoke) {
    Task { @MainActor in
      guard let scene = self.scene() else {
        invoke.reject("RATA has no window to show your subscriptions in.")
        return
      }
      do {
        try await AppStore.showManageSubscriptions(in: scene)
        invoke.resolve()
      } catch {
        invoke.reject(error.localizedDescription)
      }
    }
  }

  @objc public func finish(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(IdArgs.self)
    Task {
      guard let finished = await self.store.finish(args.id) else {
        invoke.reject("That is not a transaction.")
        return
      }
      invoke.resolve(finished)
    }
  }
}

/// Before iOS 15 there is no StoreKit 2, and RATA does not run there
/// (tauri.ios.conf.json). This only keeps the package building whatever
/// deployment target the build hands the Swift compiler: every command
/// is refused as one this plugin does not have.
class NoStoreKit: Plugin {}

@_cdecl("init_plugin_storekit")
func initPlugin() -> Plugin {
  if #available(iOS 15.0, *) {
    return StoreKitPlugin()
  }
  return NoStoreKit()
}
