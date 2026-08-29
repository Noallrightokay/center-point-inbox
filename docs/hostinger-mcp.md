# Hostinger MCP Servers

Center Point Inbox is deployed on Hostinger. These MCP servers let Claude Code
manage that infrastructure — hosting, domains, DNS, billing, VPS, and store —
directly from the repo, instead of clicking through hPanel.

All six servers ship in a single npm package,
[`hostinger-api-mcp`](https://github.com/hostinger/api-mcp-server). Each one is
a separate binary that exposes only the tools for its slice of the API, which
keeps the tool list small enough to stay useful.

---

## 1. Get an API token

1. Sign in to [hPanel](https://hpanel.hostinger.com/).
2. Go to **Account → API** and create a new token.
3. Grant it only the scopes you actually need — a token with VPS write access
   can destroy servers.

The token is a live production credential. Never commit it, never paste it into
an issue or PR, and rotate it in hPanel if it leaks.

## 2. Store the token

Add it to your local `.env` (already git-ignored):

```bash
HOSTINGER_API_TOKEN=your-real-token
```

`.mcp.json` at the repo root references `${HOSTINGER_API_TOKEN}` rather than
embedding the value, so the config is safe to commit and every contributor
supplies their own token.

Claude Code does not read `.env` on its own. Export the variable in the shell
you launch `claude` from, or put it in your shell profile:

```bash
# macOS / Linux
export HOSTINGER_API_TOKEN=your-real-token
```

```powershell
# Windows (PowerShell) — persists for future sessions
setx HOSTINGER_API_TOKEN "your-real-token"
```

## 3. Approve the project config

`.mcp.json` is project-scoped, so Claude Code prompts for approval the first
time you open the repo. Accept it, then confirm the servers are connected:

```
/mcp
```

You should see `hostinger-hosting`, `hostinger-domains`, `hostinger-dns`,
`hostinger-billing`, `hostinger-vps`, and `hostinger-ecommerce`.

---

## Windows: user-scoped alternative

The committed `.mcp.json` uses `npx` so it works on macOS, Linux, and Windows
alike. If Windows fails to launch `npx` (some setups only resolve the
`npx.cmd` shim), configure the servers per-user instead of per-project in
`%USERPROFILE%\.claude.json`:

```json
{
  "mcpServers": {
    "hostinger-hosting": {
      "command": "npx.cmd",
      "args": ["--package=hostinger-api-mcp@latest", "hostinger-hosting-mcp"],
      "env": { "HOSTINGER_API_TOKEN": "your-token-here" }
    },
    "hostinger-domains": {
      "command": "npx.cmd",
      "args": ["--package=hostinger-api-mcp@latest", "hostinger-domains-mcp"],
      "env": { "HOSTINGER_API_TOKEN": "your-token-here" }
    },
    "hostinger-dns": {
      "command": "npx.cmd",
      "args": ["--package=hostinger-api-mcp@latest", "hostinger-dns-mcp"],
      "env": { "HOSTINGER_API_TOKEN": "your-token-here" }
    },
    "hostinger-billing": {
      "command": "npx.cmd",
      "args": ["--package=hostinger-api-mcp@latest", "hostinger-billing-mcp"],
      "env": { "HOSTINGER_API_TOKEN": "your-token-here" }
    },
    "hostinger-vps": {
      "command": "npx.cmd",
      "args": ["--package=hostinger-api-mcp@latest", "hostinger-vps-mcp"],
      "env": { "HOSTINGER_API_TOKEN": "your-token-here" }
    },
    "hostinger-ecommerce": {
      "command": "npx.cmd",
      "args": ["--package=hostinger-api-mcp@latest", "hostinger-ecommerce-mcp"],
      "env": { "HOSTINGER_API_TOKEN": "your-token-here" }
    }
  }
}
```

That file lives outside the repo, so the token in it is never committed — but
it is stored in plaintext on your machine.

---

## Other servers in the package

Beyond the six configured here, `hostinger-api-mcp` also provides
`hostinger-mail-mcp`, `hostinger-wordpress-mcp`, `hostinger-reach-mcp`,
`hostinger-horizons-mcp`, and `hostinger-agency-hosting-mcp`. Add one by
copying an existing block in `.mcp.json` and swapping the binary name.

`hostinger-api-mcp` (the package's default binary) exposes every tool from
every domain in one server. It is convenient but floods the tool list, so
prefer the focused binaries.

---

## Notes

- Requires **Node.js 20+** — the same version the frontend needs.
- `@latest` re-resolves on each launch. Pin a version
  (`hostinger-api-mcp@1.52.0`) if you want reproducible startups.
- These tools act on **live infrastructure**. Treat DNS edits, VPS actions, and
  billing calls as production changes, and read back what a tool did before
  chaining another call onto it.
