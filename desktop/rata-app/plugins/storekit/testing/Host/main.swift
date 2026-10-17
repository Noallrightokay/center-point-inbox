// The app StoreKit's test environment serves while the StoreKit tests
// (../Tests) run inside it. It shows nothing and does nothing else.

import AppKit

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
app.run()
