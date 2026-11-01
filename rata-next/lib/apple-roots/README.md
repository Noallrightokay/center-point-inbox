# Apple's root certificates

The roots `lib/apple.js` gives Apple's `SignedDataVerifier`, which checks
that a purchase or notification from the App Store was signed by Apple. They
are the three in the *Apple Root Certificates* section of
<https://www.apple.com/certificateauthority/>, as the library's
documentation asks; the App Store signs under Apple Root CA - G3.

| File | Certificate | SHA-256 fingerprint |
|---|---|---|
| `AppleRootCA-G3.cer` | Apple Root CA - G3 | `63:34:3A:BF:B8:9A:6A:03:EB:B5:7E:9B:3F:5F:A7:BE:7C:4F:5C:75:6F:30:17:B3:A8:C4:88:C3:65:3E:91:79` |
| `AppleRootCA-G2.cer` | Apple Root CA - G2 | `C2:B9:B0:42:DD:57:83:0E:7D:11:7D:AC:55:AC:8A:E1:94:07:D3:8E:41:D8:8F:32:15:BC:3A:89:04:44:A0:50` |
| `AppleIncRootCertificate.cer` | Apple Root CA | `B0:B1:73:0E:CB:C7:FF:45:05:14:2C:49:F1:29:5E:6E:DA:6B:CA:ED:7E:2C:68:C5:BE:91:B5:A1:10:01:F0:24` |

Downloaded from the links on that page on 2026-10-15 and checked against
the fingerprints Apple publishes in its lists of trusted roots
(<https://support.apple.com/en-us/121672>, and
<https://support.apple.com/en-us/105116> before it). `tests/apple.test.mjs`
pins them: a file changed or added here fails the suite.
