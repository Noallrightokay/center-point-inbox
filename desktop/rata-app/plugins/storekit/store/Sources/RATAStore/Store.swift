// RATA's In-App Purchase, with StoreKit 2: what the App Store sells RATA
// as, what a purchase ended as, and what the customer is entitled to now.
//
// This is the part that talks to StoreKit, and only that: no Tauri, no
// UIKit, no AppKit. The iPhone app's Tauri plugin (../../../ios) wraps it,
// and a later Mac App Store build can wrap the same code, since StoreKit 2
// is one API on iOS 15 and macOS 12. What needs a window is the wrapper's:
// it says how to show the purchase sheet (`purchase`'s `buying`), and shows
// Apple's subscriptions sheet itself.
//
// RATA's Rust decides everything else (src-tauri/src/appstore.rs): which
// product a plan is, when mailrata.org is asked for a licence, and when a
// transaction is finished, which is only once that licence is kept. The
// answers below are what it reads, as JSON, whichever wrapper hands them
// over.
//
// Only a transaction StoreKit verified (`VerificationResult.verified`) ever
// leaves here, each with Apple's signed record of it (its
// `jwsRepresentation`), which mailrata.org checks again. An unverified one
// is never handed over as bought.

import Foundation
import StoreKit

/// One verified transaction: Apple's signed record of it (`jws`), which RATA
/// sends to mailrata.org, and the id RATA finishes it by once the licence it
/// brought is kept.
public struct Signed: Encodable {
  public let id: String
  public let originalId: String
  public let product: String
  public let jws: String
  public let revoked: Bool
}

public struct Period: Encodable {
  public let unit: String
  public let value: Int
}

/// An introductory offer the customer can still take: its terms, which the
/// page must show beside the price.
public struct Intro: Encodable {
  public let displayPrice: String
  public let period: Period
  public let periodCount: Int
  public let paymentMode: String
}

/// A product as the App Store offers it here, with the price in the
/// customer's own currency. RATA never writes a price of its own.
public struct Listed: Encodable {
  public let id: String
  public let displayName: String
  public let description: String
  public let displayPrice: String
  public let period: Period?
  public let intro: Intro?
}

public struct ProductsAnswer: Encodable {
  public let products: [Listed]
}

/// `outcome` is bought, pending, cancelled, failed or unverified; only
/// bought carries a transaction.
public struct PurchaseAnswer: Encodable {
  public let outcome: String
  public let transaction: Signed?
  public let message: String?
}

public struct Entitled: Encodable {
  public let transactions: [Signed]
}

public struct Restored: Encodable {
  public let cancelled: Bool
  public let transactions: [Signed]
}

public struct Finished: Encodable {
  public let finished: Bool
}

@available(iOS 15.0, macOS 12.0, *)
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

@available(iOS 15.0, macOS 12.0, *)
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

@available(iOS 15.0, macOS 12.0, *)
private func paymentMode(_ mode: Product.SubscriptionOffer.PaymentMode) -> String {
  if mode == .freeTrial { return "free-trial" }
  if mode == .payAsYouGo { return "pay-as-you-go" }
  if mode == .payUpFront { return "pay-up-front" }
  return "other"
}

@available(iOS 15.0, macOS 12.0, *)
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

/// StoreKit, as RATA asks it. One for the life of the app: it holds the
/// verified transactions seen this run, for `finish`, and listens to
/// `Transaction.updates` once `start` is called.
@available(iOS 15.0, macOS 12.0, *)
public final class Store {
  private let lock = NSLock()
  /// Verified transactions seen this run, by id, for `finish`.
  private var held: [UInt64: Transaction] = [:]
  /// Where `Transaction.updates` goes: RATA's Rust, once it has said so.
  private var forward: ((Signed) -> Void)?
  /// What `Transaction.updates` brought before Rust was listening: at
  /// launch StoreKit hands over every transaction not yet finished.
  private var waiting: [Signed] = []
  private var updates: Task<Void, Never>?

  public init() {}

  /// Listens to `Transaction.updates`, as Apple asks, from as early as the
  /// app can: a renewal, an Ask to Buy approved, a purchase made on another
  /// device, and at launch each transaction not yet finished. Each verified
  /// one goes to `listen`'s receiver; an unverified one goes nowhere. A
  /// second call changes nothing.
  public func start() {
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

  /// Whether `start` has been called: the listener is running.
  public var listening: Bool {
    lock.lock()
    defer { lock.unlock() }
    return updates != nil
  }

  /// Where each verified transaction from `Transaction.updates` goes from
  /// now on, after what came before it (at most 32 are kept waiting).
  public func listen(_ receiver: @escaping (Signed) -> Void) {
    lock.lock()
    forward = receiver
    let early = waiting
    waiting = []
    lock.unlock()
    for signed in early {
      receiver(signed)
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
    let receiver = forward
    if receiver == nil && waiting.count < 32 {
      waiting.append(signed)
    }
    lock.unlock()
    receiver?(signed)
  }

  /// The products, as the App Store offers them here. StoreKit leaves out
  /// any it does not know, so an empty list is an answer too.
  public func products(_ ids: [String]) async throws -> ProductsAnswer {
    var listed: [Listed] = []
    for product in try await Product.products(for: ids) {
      listed.append(await Listed(product))
    }
    return ProductsAnswer(products: listed)
  }

  /// Buys the product `id`. `buying` shows the purchase sheet: by default
  /// StoreKit's own choice of window; a wrapper that can name RATA's window
  /// passes `purchase(confirmIn:)` with it.
  public func purchase(
    _ id: String,
    buying: (Product) async throws -> Product.PurchaseResult = { try await $0.purchase() }
  ) async -> PurchaseAnswer {
    do {
      guard let product = try await Product.products(for: [id]).first else {
        return PurchaseAnswer(
          outcome: "failed", transaction: nil,
          message: "The App Store does not offer this subscription here.")
      }
      switch try await buying(product) {
      case .success(let result):
        guard case .verified(let transaction) = result else {
          return PurchaseAnswer(outcome: "unverified", transaction: nil, message: nil)
        }
        hold(transaction)
        return PurchaseAnswer(
          outcome: "bought",
          transaction: Signed(transaction, jws: result.jwsRepresentation),
          message: nil)
      case .pending:
        return PurchaseAnswer(outcome: "pending", transaction: nil, message: nil)
      case .userCancelled:
        return PurchaseAnswer(outcome: "cancelled", transaction: nil, message: nil)
      @unknown default:
        return PurchaseAnswer(
          outcome: "failed", transaction: nil,
          message: "The App Store answered in a way RATA does not know.")
      }
    } catch StoreKitError.userCancelled {
      return PurchaseAnswer(outcome: "cancelled", transaction: nil, message: nil)
    } catch {
      return PurchaseAnswer(outcome: "failed", transaction: nil, message: error.localizedDescription)
    }
  }

  /// Each verified transaction the customer is entitled to now. It never
  /// asks the customer anything, so it may run at launch.
  public func entitlements() async -> Entitled {
    var list: [Signed] = []
    for await result in Transaction.currentEntitlements {
      if case .verified(let transaction) = result {
        hold(transaction)
        list.append(Signed(transaction, jws: result.jwsRepresentation))
      }
    }
    return Entitled(transactions: list)
  }

  /// Restore purchases: the App Store's own sync, which may ask the
  /// customer to sign in, so only ever on their asking; then what they are
  /// entitled to. Any failure but their cancelling is thrown.
  public func restore() async throws -> Restored {
    do {
      try await AppStore.sync()
    } catch StoreKitError.userCancelled {
      return Restored(cancelled: true, transactions: [])
    }
    return Restored(cancelled: false, transactions: await entitlements().transactions)
  }

  /// Finishes a verified transaction, which Rust asks only once the licence
  /// it brought is kept. `finished` is false when there was none by that id
  /// still waiting; nil when `id` is not a transaction's id at all.
  public func finish(_ id: String) async -> Finished? {
    guard let id = UInt64(id) else { return nil }
    if let transaction = take(id) {
      await transaction.finish()
      return Finished(finished: true)
    }
    for await result in Transaction.unfinished {
      if case .verified(let transaction) = result, transaction.id == id {
        await transaction.finish()
        return Finished(finished: true)
      }
    }
    return Finished(finished: false)
  }
}
