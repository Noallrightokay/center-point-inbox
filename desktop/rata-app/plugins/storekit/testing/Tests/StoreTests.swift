// RATAStore against ios/RATA.storekit, the App Store's two subscriptions at
// placeholder prices, in StoreKit's test environment (project.yml says
// why in a host app). What RATA's Rust then does with each answer is
// tested in src-tauri/src/appstore.rs, against a stand-in of this.

import Foundation
import RATAStore
import StoreKit
import StoreKitTest
import XCTest

let pro = "org.mailrata.pro.monthly"
let base = "org.mailrata.base.monthly"

/// What a receiver passed to `listen` heard, from whichever thread.
final class Heard {
  private let lock = NSLock()
  private var list: [Signed] = []

  func add(_ signed: Signed) {
    lock.lock()
    list.append(signed)
    lock.unlock()
  }

  var all: [Signed] {
    lock.lock()
    defer { lock.unlock() }
    return list
  }
}

final class StoreTests: XCTestCase {
  var session: SKTestSession!

  override func setUpWithError() throws {
    let config = try XCTUnwrap(
      Bundle(for: StoreTests.self).url(forResource: "RATA", withExtension: "storekit"),
      "RATA.storekit is not in the tests' bundle")
    session = try SKTestSession(contentsOf: config)
    session.resetToDefaultState()
    session.disableDialogs = true
    session.clearTransactions()
  }

  override func tearDown() {
    session?.clearTransactions()
    session = nil
  }

  /// Waits until `heard` holds something, at most `seconds`.
  func arrived(in heard: Heard, seconds: Double = 30) async -> [Signed] {
    let until = Date().addingTimeInterval(seconds)
    while heard.all.isEmpty && Date() < until {
      try? await Task.sleep(nanoseconds: 200_000_000)
    }
    return heard.all
  }

  /// StoreKit reads the configuration, and sells exactly RATA's two
  /// products from it, monthly, at its prices, with no offer.
  func testTheProductsAreRatasTwoMonthlySubscriptions() async throws {
    let answer = try await Store().products([pro, base, "org.mailrata.gold.monthly"])
    let byId = Dictionary(uniqueKeysWithValues: answer.products.map { ($0.id, $0) })
    XCTAssertEqual(Set(byId.keys), [pro, base])
    XCTAssertEqual(byId[pro]?.displayName, "RATA Pro")
    XCTAssertEqual(byId[base]?.displayName, "RATA Base")
    XCTAssertTrue(byId[pro]?.displayPrice.contains("1.99") == true, byId[pro]?.displayPrice ?? "")
    XCTAssertTrue(byId[base]?.displayPrice.contains("0.99") == true, byId[base]?.displayPrice ?? "")
    for listed in answer.products {
      XCTAssertEqual(listed.period?.unit, "month", listed.id)
      XCTAssertEqual(listed.period?.value, 1, listed.id)
      XCTAssertNil(listed.intro, listed.id)
    }
  }

  /// Bought: Apple's signed record comes with it, it is what the customer
  /// is entitled to, and it is finished once, when RATA asks.
  func testBoughtCarriesItsSignedRecordAndIsFinishedOnce() async throws {
    let store = Store()
    let answer = await store.purchase(pro)
    XCTAssertEqual(answer.outcome, "bought", answer.message ?? "")
    let bought = try XCTUnwrap(answer.transaction)
    XCTAssertEqual(bought.product, pro)
    XCTAssertEqual(bought.jws.split(separator: ".").count, 3)
    XCTAssertFalse(bought.revoked)
    XCTAssertFalse(bought.originalId.isEmpty)

    let entitled = await store.entitlements().transactions
    XCTAssertEqual(entitled.map(\.id), [bought.id])
    XCTAssertEqual(entitled.first?.jws.split(separator: ".").count, 3)

    let first = await store.finish(bought.id)
    XCTAssertEqual(first?.finished, true)
    let again = await store.finish(bought.id)
    XCTAssertEqual(again?.finished, false)
    let notOne = await store.finish("not a transaction")
    XCTAssertNil(notOne)
  }

  /// A product the App Store does not sell, and a purchase that fails, are
  /// failed with a sentence, and nothing is bought.
  func testFailedSaysWhyAndBuysNothing() async throws {
    let store = Store()
    let unknown = await store.purchase("org.mailrata.gold.monthly")
    XCTAssertEqual(unknown.outcome, "failed")
    XCTAssertEqual(unknown.message, "The App Store does not offer this subscription here.")
    XCTAssertNil(unknown.transaction)

    session.failTransactionsEnabled = true
    session.failureError = .unknown
    let failed = await store.purchase(pro)
    XCTAssertEqual(failed.outcome, "failed")
    XCTAssertNil(failed.transaction)
    XCTAssertFalse((failed.message ?? "").isEmpty)
    let entitled = await store.entitlements().transactions
    XCTAssertTrue(entitled.isEmpty)
  }

  /// Ask to Buy: the purchase is pending and carries nothing; once it is
  /// approved, the transaction reaches the listener through
  /// Transaction.updates, as RATA's Rust hears it.
  func testAskToBuyIsPendingThenReachesTheListener() async throws {
    session.askToBuyEnabled = true
    let store = Store()
    store.start()
    XCTAssertTrue(store.listening)
    let heard = Heard()
    store.listen { heard.add($0) }

    let answer = await store.purchase(base)
    XCTAssertEqual(answer.outcome, "pending", answer.message ?? "")
    XCTAssertNil(answer.transaction)

    let waiting = try XCTUnwrap(session.allTransactions().first)
    try session.approveAskToBuyTransaction(identifier: waiting.identifier)
    let got = await arrived(in: heard)
    XCTAssertEqual(got.first?.product, base)
    XCTAssertEqual(got.first?.jws.split(separator: ".").count, 3)
    let id = try XCTUnwrap(got.first?.id)
    let finished = await store.finish(id)
    XCTAssertEqual(finished?.finished, true)
  }

  /// A purchase made outside the app (on another device; here, by the test
  /// session) arrives through Transaction.updates whether or not anything
  /// listens yet, and is handed to the listener when there is one.
  func testWhatArrivesBeforeAnyListenerWaitsForIt() async throws {
    let store = Store()
    store.start()
    try session.buyProduct(productIdentifier: pro)
    try? await Task.sleep(nanoseconds: 3_000_000_000)
    let heard = Heard()
    store.listen { heard.add($0) }
    let got = await arrived(in: heard)
    XCTAssertEqual(got.map(\.product), [pro])
  }

  /// A refund reaches the listener marked revoked, and leaves nothing the
  /// customer is entitled to.
  func testARefundIsHeardAsRevokedAndEntitlesToNothing() async throws {
    let store = Store()
    let answer = await store.purchase(pro)
    let bought = try XCTUnwrap(answer.transaction, answer.message ?? answer.outcome)
    let finished = await store.finish(bought.id)
    XCTAssertEqual(finished?.finished, true)

    store.start()
    let heard = Heard()
    store.listen { heard.add($0) }
    try session.refundTransaction(identifier: try XCTUnwrap(UInt(bought.id)))
    let got = await arrived(in: heard)
    XCTAssertEqual(got.first?.id, bought.id)
    XCTAssertEqual(got.first?.revoked, true)
    let entitled = await store.entitlements().transactions
    XCTAssertTrue(entitled.isEmpty)
  }
}
