// RATA's In-App Purchase on iOS, with StoreKit 2.
//
// RATA's Rust asks this (src-tauri/src/appstore.rs) and decides everything
// itself: which product a plan is, when mailrata.org is asked for a licence,
// and when a transaction is finished, which is only once that licence is
// kept. This side only does what needs StoreKit, and says what happened.
// The page cannot reach it: no permission of Tauri's allows any of it.
//
// Only a transaction StoreKit verified (`VerificationResult.verified`) ever
// leaves here as bought, each with Apple's signed record of it (its
// `jwsRepresentation`), which mailrata.org checks again. An unverified one is
// never forwarded.
//
// Every command answers from a task of its own: Tauri hands commands over on
// one queue, and a purchase can wait on the customer for minutes.

import Foundation
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

/// One verified transaction: Apple's signed record of it (`jws`), which RATA
/// sends to mailrata.org, and the id RATA finishes it by once the licence it
/// brought is kept.
struct Signed: Encodable {
  let id: String
  let originalId: String
  let product: String
  let jws: String
  let revoked: Bool
}

struct Period: Encodable {
  let unit: String
  let value: Int
}

/// An introductory offer the customer can still take: its terms, which the
/// page must show beside the price.
struct Intro: Encodable {
  let displayPrice: String
  let period: Period
  let periodCount: Int
  let paymentMode: String
}

/// A product as the App Store offers it here, with the price in the
/// customer's own currency. RATA never writes a price of its own.
struct Listed: Encodable {
  let id: String
  let displayName: String
  let description: String
  let displayPrice: String
  let period: Period?
  let intro: Intro?
}

struct ProductsAnswer: Encodable {
  let products: [Listed]
}

/// `outcome` is bought, pending, cancelled, failed or unverified; only
/// bought carries a transaction.
struct PurchaseAnswer: Encodable {
  let outcome: String
  let transaction: Signed?
  let message: String?
}

struct Entitled: Encodable {
  let transactions: [Signed]
}

struct Restored: Encodable {
  let cancelled: Bool
  let transactions: [Signed]
}

struct Finished: Encodable {
  let finished: Bool
}

@available(iOS 15.0, *)
extension Signed {
  init(_ transaction: Transaction, jws: String) {
    self.init(
      id: String(transaction.id),
      originalId: String(transaction.originalID),
      product: transaction.productID,
      jws: jws,
      revoked: transaction.revocationDate != nil)
  }
}

@available(iOS 15.0, *)
extension Period {
  init(_ period: Product.SubscriptionPeriod) {
    let unit: String
    switch period.unit {
    case .day: unit = "day"
    case .week: unit = "week"
    case .month: unit = "month"
    case .year: unit = "year"
    @unknown default: unit = "unknown"
    }
    self.init(unit: unit, value: period.value)
  }
}

@available(iOS 15.0, *)
private func paymentMode(_ mode: Product.SubscriptionOffer.PaymentMode) -> String {
  if mode == .freeTrial { return "free-trial" }
  if mode == .payAsYouGo { return "pay-as-you-go" }
  if mode == .payUpFront { return "pay-up-front" }
  return "other"
}

@available(iOS 15.0, *)
extension Listed {
  init(_ product: Product) async {
    var intro: Intro? = nil
    if let subscription = product.subscription, let offer = subscription.introductoryOffer {
      let eligible = await subscription.isEligibleForIntroOffer
      if eligible {
        intro = Intro(
          displayPrice: offer.displayPrice,
          period: Period(offer.period),
          periodCount: offer.periodCount,
          paymentMode: paymentMode(offer.paymentMode))
      }
    }
    self.init(
      id: product.id,
      displayName: product.displayName,
      description: product.description,
      displayPrice: product.displayPrice,
      period: product.subscription.map { Period($0.subscriptionPeriod) },
      intro: intro)
  }
}

@available(iOS 15.0, *)
class StoreKitPlugin: Plugin {
  private let lock = NSLock()
  /// Verified transactions seen this run, by id, for `finish`.
  private var held: [UInt64: Transaction] = [:]
  /// Where `Transaction.updates` goes: RATA's Rust, once it has said so.
  private var listener: Channel?
  /// What `Transaction.updates` brought before Rust was listening: at
  /// launch StoreKit hands over every transaction not yet finished.
  private var waiting: [Signed] = []
  private var updates: Task<Void, Never>?

  /// Listens to `Transaction.updates` from the moment the plugin loads, as
  /// Apple asks: a renewal, an Ask to Buy approved, a purchase made on
  /// another device, and at launch each transaction not yet finished. Each
  /// verified one goes to Rust; an unverified one goes nowhere.
  @objc public override func load(webview: WKWebView) {
    lock.lock()
    defer { lock.unlock() }
    if updates != nil { return }
    updates = Task.detached(priority: .background) { [weak self] in
      for await result in Transaction.updates {
        guard case .verified(let transaction) = result else { continue }
        self?.heard(transaction, jws: result.jwsRepresentation)
      }
    }
  }

  private func hold(_ transaction: Transaction) {
    lock.lock()
    held[transaction.id] = transaction
    lock.unlock()
  }

  private func take(_ id: UInt64) -> Transaction? {
    lock.lock()
    defer { lock.unlock() }
    return held.removeValue(forKey: id)
  }

  private func heard(_ transaction: Transaction, jws: String) {
    let signed = Signed(transaction, jws: jws)
    lock.lock()
    held[transaction.id] = transaction
    let channel = listener
    if channel == nil && waiting.count < 32 {
      waiting.append(signed)
    }
    lock.unlock()
    if let channel = channel {
      try? channel.send(signed)
    }
  }

  /// Rust's channel for `Transaction.updates`, and what came before it.
  @objc public func listen(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ListenArgs.self)
    lock.lock()
    listener = args.handler
    let early = waiting
    waiting = []
    lock.unlock()
    for signed in early {
      try? args.handler.send(signed)
    }
    invoke.resolve()
  }

  @objc public func products(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ProductsArgs.self)
    Task {
      do {
        var listed: [Listed] = []
        for product in try await Product.products(for: args.ids) {
          listed.append(await Listed(product))
        }
        invoke.resolve(ProductsAnswer(products: listed))
      } catch {
        invoke.reject(error.localizedDescription)
      }
    }
  }

  @objc public func purchase(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(IdArgs.self)
    Task { @MainActor in
      do {
        guard let product = try await Product.products(for: [args.id]).first else {
          invoke.resolve(
            PurchaseAnswer(
              outcome: "failed", transaction: nil,
              message: "The App Store does not offer this subscription here."))
          return
        }
        switch try await self.buy(product) {
        case .success(let result):
          if case .verified(let transaction) = result {
            self.hold(transaction)
            invoke.resolve(
              PurchaseAnswer(
                outcome: "bought",
                transaction: Signed(transaction, jws: result.jwsRepresentation),
                message: nil))
          } else {
            invoke.resolve(PurchaseAnswer(outcome: "unverified", transaction: nil, message: nil))
          }
        case .pending:
          invoke.resolve(PurchaseAnswer(outcome: "pending", transaction: nil, message: nil))
        case .userCancelled:
          invoke.resolve(PurchaseAnswer(outcome: "cancelled", transaction: nil, message: nil))
        @unknown default:
          invoke.resolve(
            PurchaseAnswer(
              outcome: "failed", transaction: nil,
              message: "The App Store answered in a way RATA does not know."))
        }
      } catch StoreKitError.userCancelled {
        invoke.resolve(PurchaseAnswer(outcome: "cancelled", transaction: nil, message: nil))
      } catch {
        invoke.resolve(
          PurchaseAnswer(outcome: "failed", transaction: nil, message: error.localizedDescription))
      }
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

  /// Each verified transaction the customer is entitled to now.
  private func entitled() async -> [Signed] {
    var list: [Signed] = []
    for await result in Transaction.currentEntitlements {
      if case .verified(let transaction) = result {
        hold(transaction)
        list.append(Signed(transaction, jws: result.jwsRepresentation))
      }
    }
    return list
  }

  @objc public func entitlements(_ invoke: Invoke) {
    Task {
      invoke.resolve(Entitled(transactions: await self.entitled()))
    }
  }

  /// Restore purchases: the App Store's own sync (which may ask the customer
  /// to sign in), then what they are entitled to.
  @objc public func restore(_ invoke: Invoke) {
    Task {
      do {
        try await AppStore.sync()
      } catch StoreKitError.userCancelled {
        invoke.resolve(Restored(cancelled: true, transactions: []))
        return
      } catch {
        invoke.reject(error.localizedDescription)
        return
      }
      invoke.resolve(Restored(cancelled: false, transactions: await self.entitled()))
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

  /// Finishes a verified transaction, which Rust asks only once the licence
  /// it brought is kept. `finished` is false when there was none by that id
  /// still waiting.
  @objc public func finish(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(IdArgs.self)
    guard let id = UInt64(args.id) else {
      invoke.reject("That is not a transaction.")
      return
    }
    Task {
      if let transaction = self.take(id) {
        await transaction.finish()
        invoke.resolve(Finished(finished: true))
        return
      }
      for await result in Transaction.unfinished {
        if case .verified(let transaction) = result, transaction.id == id {
          await transaction.finish()
          invoke.resolve(Finished(finished: true))
          return
        }
      }
      invoke.resolve(Finished(finished: false))
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
