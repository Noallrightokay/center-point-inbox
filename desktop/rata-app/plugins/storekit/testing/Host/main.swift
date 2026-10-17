// The app StoreKit's test environment serves while the StoreKit tests
// (../Tests) run inside it. It does nothing itself: the tests open the
// window each purchase is anchored to.

import AppKit

let app = NSApplication.shared
app.setActivationPolicy(.regular)
app.run()
