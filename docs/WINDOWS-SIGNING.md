# Signing the Windows installer

Without a signature, Windows SmartScreen stops RATA's installer with
"Windows protected your PC" and hides **Run anyway** behind **More info**.
`release.yml` signs the Windows build when it finds the secrets below, and
builds it unsigned, exactly as before, when it finds none. This page is the
owner's side: what to buy, which secrets to add, and how to tell it worked.

Nothing here goes in the repository. Every value below goes in
**Settings → Secrets and variables → Actions** on GitHub.

## What gets signed

Tauri's bundler does the signing, during the build, so it covers everything
the installer is made of:

- `rata-app.exe`, the program the installer puts on the customer's disk;
- NSIS's plugin DLLs and the uninstaller;
- `RATA_<version>_x64-setup.exe`, the installer itself.

It has to be the bundler: the installer is uploaded to the release the
moment it is built, and the update signature (`.sig`, see LAUNCH.md §9) is
made over the installer as it is then. Signing afterwards would ship an
unsigned installer, or break every update.

## Pick one of two ways

Set up one. If the secrets of both are present the build stops and says so,
so that there is never a doubt which certificate a release carries.

### A. Azure Artifact Signing (recommended)

Microsoft's signing service, formerly called Trusted Signing. The private
key stays in Microsoft's hardware; GitHub holds only a login that may ask
for signatures. It costs about $10 a month (Basic); check the current price
and who is eligible (Microsoft limits it to organisations and individual
developers in certain countries) before starting.

1. In the Azure portal, create an **Artifact Signing account** (it may still
   be listed as Trusted Signing). Note its **region**: it decides the
   endpoint, e.g. West Europe is `https://weu.codesigning.azure.net`, East US
   is `https://eus.codesigning.azure.net`. A wrong region is the usual cause
   of a `403 Forbidden` while signing.
2. Complete **identity validation** for the name the certificate will carry
   (the publisher customers see).
3. Create a **certificate profile** of type **Public Trust**.
4. In Microsoft Entra ID, **register an application** (App registrations →
   New registration; no redirect URI). Under Certificates & secrets, add a
   **client secret** and copy its *value*. Note its expiry date: when it
   expires, signing stops with an authentication error until a new one is
   put in the secret below.
5. On the signing account (Access control (IAM) → Add role assignment),
   give that application the **Certificate Profile Signer** role (named
   "Trusted Signing Certificate Profile Signer" or "Artifact Signing
   Certificate Profile Signer", depending on when you look).
6. Add to GitHub:

| Name | Where | Value |
|---|---|---|
| `AZURE_TENANT_ID` | secret | Directory (tenant) ID of the app registration |
| `AZURE_CLIENT_ID` | secret | Application (client) ID of the app registration |
| `AZURE_CLIENT_SECRET` | secret | the client secret's value |
| `AZURE_ENDPOINT` | variable (or secret) | e.g. `https://weu.codesigning.azure.net` |
| `AZURE_CODE_SIGNING_NAME` | variable (or secret) | the signing account's name |
| `AZURE_CERT_PROFILE_NAME` | variable (or secret) | the certificate profile's name |

The last three are not secret. Prefer **variables**: GitHub hides every
secret's value wherever it appears in a log, and an account named, say,
`rata` would turn every "rata" in the build log into `***`.

All six must be set. With some but not all, the Windows build fails and
names the missing ones.

How it runs: the workflow downloads Microsoft's signing client
(`Microsoft.ArtifactSigning.Client`, pinned by version and SHA-256), writes
its `metadata.json`, and gives Tauri a `signCommand` that runs the Windows
SDK's `signtool` with that client and Microsoft's timestamp server. Only the
client secret is used to sign in; every other Azure login method is turned
off in the metadata.

### B. A certificate file (.pfx)

Only for a code-signing certificate you already hold as a `.pfx` file with
its private key. Since June 2023 certificate authorities issue new OV and EV
code-signing certificates only on hardware tokens or cloud HSMs, which cannot
be exported to a `.pfx`, so a newly bought certificate will usually not work
this way; use A.

1. Encode the file as one line of base64, on your own machine:
   - macOS / Linux: `base64 -i certificate.pfx | tr -d '\n' > certificate.b64`
   - Windows PowerShell:
     `[Convert]::ToBase64String([IO.File]::ReadAllBytes("certificate.pfx")) | Set-Content certificate.b64`
2. Add to GitHub, both as **secrets**:

| Name | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | the contents of `certificate.b64` |
| `WINDOWS_CERTIFICATE_PASSWORD` | the `.pfx` export password (a `.pfx` without a password is not supported) |

3. Delete `certificate.b64`.

How it runs: the certificate is decoded in memory and imported into the
build machine's certificate store (it is never written out as a file), and
Tauri signs with its built-in `signtool` call by the certificate's
thumbprint, timestamped by DigiCert's RFC 3161 server. The build refuses a
`.pfx` with no code-signing certificate and private key in it, or one that
has expired. At the end of the job the certificate and its key are removed
from the store.

## How to tell it worked

In the release run, on the **Windows (.exe)** job:

| Step | Unsigned (no secrets) | Signed |
|---|---|---|
| Configure Windows signing | `No Windows signing secrets - building unsigned.` and a warning | `Signing Windows binaries with Azure Artifact Signing (...)` or `... with the certificate for CN=... (thumbprint ..., valid until ...)` |
| tauri-action | no `Signing` lines | `Signing ... rata-app.exe`, `...-setup.exe` |
| Check the Windows signatures | `Unsigned build: no Windows signing secrets are set, so there is no signature to check.` | `signtool verify /pa /v` for the installer and the `rata-app.exe` inside it, then `Every Windows binary checked is signed, trusted and timestamped` |

If the check fails, the job fails, and the Windows installer (and its update
signature) is taken off the draft release before it is published, so a
badly signed installer never ships. It stays on the run as an artifact for
looking into. The other platforms are published as usual.

To check a downloaded installer yourself on Windows: right-click →
Properties → **Digital Signatures** should list the publisher, or from a
Developer Command Prompt:

```
signtool verify /pa /v RATA_<version>_x64-setup.exe
```

## What signing does and does not do

A signature names the publisher and proves the file was not changed.
SmartScreen also weighs reputation, which a new certificate has to earn: the
first signed releases may still show a warning, naming RATA's publisher
instead of "Unknown publisher", until enough people have installed them.
Signing every release with the same identity is what builds it.

Rotating or removing: replace the secrets and the next release uses the new
ones; delete them all and the next release is unsigned again, with a
warning in the log.
