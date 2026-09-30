# RATA design system

How RATA looks, for anyone (or any agent) changing the interface
(`rata-next/public/app.html`, which the desktop app builds from) or the
website (`index.html`, `auth.html`, `account.html`). The tokens below are the
CSS variables at the top of each page; change them there, not per component.

## The read

A mail client people keep open all day, for people who chose it because their
mail stays on their machine. Calm, exact, and plain-spoken. Nothing on screen
should glow, bounce or compete with the mail. On the website, the same
restraint: say what RATA does, show the real app, and ask once.

| | Variance | Motion | Density |
|---|---|---|---|
| App | 3 (predictable) | 2 (hover and press only) | 6 (a working tool) |
| Website | 6 (offset, not centred) | 3 | 4 |

## Colour

Neutral base, one accent. The accent is cobalt: the brand blue, taken from
electric (`#2E5CFF`, 100 % saturation) down to something that can sit next to
mail all day. It marks the one primary action on a screen, the current place,
selection and focus. Nothing else is blue. Red, green and amber are for state
only (errors, delete, sent, warnings), never decoration.

Light:

| Token | Value | Use | Contrast on surface |
|---|---|---|---|
| `--bg` | `#F5F6F8` | window background | |
| `--card` | `#FCFCFD` | panels, cards, reading pane | |
| `--fill` | `#ECEEF1` | control backgrounds, chips | |
| `--fill-2` | `#F1F2F4` | hover, quiet wells | |
| `--ink` | `#15171C` | text | 17.5 |
| `--slate` | `#51565F` | secondary text | 7.2 |
| `--faint` | `#6B717B` | tertiary text, timestamps, on `--card` or `--bg` only | 4.8 |
| `--tint` | `#2B4FC7` | accent | 6.7 (and white on it 6.7) |
| `--tint-deep` | `#233FA3` | accent text on `--tint-soft` | |
| `--tint-btn` | `#2B4FC7`, hover `#233FA3` | filled buttons | |
| `--tint-soft` | `rgba(43,79,199,.09)` | selection, current place | |
| `--red` / `--green` / `--amber` | `#B4262D` / `#17744A` / `#8A5B0C` | state | 6.3 / 5.6 / 5.7 |
| `--hair` | `rgba(21,23,28,.10)` | borders, dividers | |

Dark (`prefers-color-scheme: dark`):

| Token | Value | Contrast on surface |
|---|---|---|
| `--bg` | `#0E1014` | |
| `--card` | `#15181D` | |
| `--fill` | `#1F232A` | |
| `--ink` | `#E7E9EE` | 14.7 |
| `--slate` | `#A4AAB5` | 7.6 |
| `--faint` | `#838A96` | 5.1 |
| `--tint` | `#8FA6F2` (text, rings) | 7.5 |
| `--tint-btn` | `#3E5FD6` (filled buttons; white text 5.4), hover `#4A6BE0` | |
| `--red` / `--green` / `--amber` | `#F07A7F` / `#4CC38A` / `#E0A94A` | 6.6 / 8.0 / 8.4 |

`--faint` holds 4.5:1 only on `--card` and `--bg`. On the selection tint or a
fill it falls short (4.16 on `--tint-soft`, 4.22 on `--fill`, 4.38 on
`--fill-2`, measured by axe in H10, v0.1.44), so text there uses `--slate`: a
selected row's time, preview and 📎, the open row's date in *In this
conversation*, the ⌘K hint, a locked item.

Never pure black or pure white. Shadows are tinted with the ink colour and
kept small; there are no coloured glows and no gradients on controls.

## Type

**Geist** for everything, **Geist Mono** for times, counts, sizes and keys
(with `font-variant-numeric: tabular-nums` where numbers line up). Both are
vendored under `rata-next/public/fonts` by `scripts/vendor-fonts.mjs`; never
link a font CDN (the app must not call Google, and CI checks the shipped
binary for it). The one exception is the **RATA wordmark**, which stays in
Fredoka 600: it is the logo, not a text style.

| Role | Size / line | Weight | Tracking |
|---|---|---|---|
| Page title (app) | 20 / 1.2 | 600 | -0.02em |
| Section heading | 15 / 1.3 | 600 | -0.01em |
| Body, list rows | 14 / 1.5 | 400 (unread 600) | 0 |
| Secondary | 13 / 1.45 | 400 | 0 |
| Label, chip | 12.5 / 1.2 | 500 | 0 |
| Micro (timestamps) | 12 / 1.2 | 400 mono | 0 |
| Site hero | clamp(36px, 5vw, 56px) / 1.05 | 600 | -0.035em |
| Site section | clamp(26px, 3vw, 34px) / 1.15 | 600 | -0.025em |

Sentence case everywhere. No uppercase letter-spaced labels except, at most,
one small section label per screen. Body text on the site stops at 62ch.

## Shape

One rule, used everywhere: **controls are 8 px, containers are 12 px, the
page is square.** Buttons, inputs, chips, menu items and list selections: 8 px.
Cards, the reading pane, dialogs, menus: 12 px. Avatars are rounded squares
(30 % radius). Nothing is a pill except the unread count and the switch.

`--thumb` (168 px) is the largest a picture thumbnail is drawn in a message
(H11): the picture keeps its proportions inside that square, with 8 px
(`--r-sm`) corners and the `--edge` hairline, and a tiny one sits in a
72 px tile.

## Elevation

Flat by default: surfaces are separated by a 1 px `--hair` border (drawn as
`--edge`, an inset shadow, so it never changes an element's size) or by the
step from `--bg` to `--card`. Shadows are for things that float: menus,
dialogs, toasts, the composer.

- `--shadow` `0 1px 2px rgba(21,23,28,.06)` (resting card, when needed)
- `--shadow-lg` `0 12px 32px -8px rgba(21,23,28,.22), 0 2px 6px rgba(21,23,28,.08)` (floating)

## Motion

Hover and press only: background/colour 120 ms ease, and a 1 px press
(`translateY(1px)`). Dialogs and menus fade and rise 4 px in 160 ms. No
springs, no looping animation, no scroll effects. Everything collapses to
instant under `prefers-reduced-motion`, including the 1 px press and every
scroll the app makes (`glide()`).

## Components

- **Primary button**: `--tint` fill, white text, 8 px, 32 px tall (36 on the
  site). One per view: Compose, Send, Reply, the site's download.
- **Secondary button**: `--card` with a 1 px `--hair` border, ink text.
- **Quiet button**: no border, `--slate` text, `--fill-2` on hover. Most
  toolbar actions.
- **Danger**: quiet button with `--red` text; filled red only to confirm.
- **Chip / filter**: transparent with an `--edge` hairline, `--slate` text;
  on is `--ink` background with `--bg` text, in both modes.
- **Input**: `--card` background, 1 px `--hair` border, 8 px, focus ring
  `0 0 0 3px var(--tint-soft)` plus a `--tint` border.
- **Focus**: every interactive element shows `:focus-visible` as a 2 px
  `--tint` outline, 2 px offset. Never remove an outline without that.
  List rows are the exception in where it sits: their ring is drawn 2 px
  *inside* (`outline-offset: -2px`), because the scrolling list clips a ring
  drawn outside.
- **List row**: no card per message, rows separated by space; selection is
  `--tint-soft`, unread is weight 600 and a 6 px `--tint` dot. Three lines
  (sender with badges and time, subject, preview), always: the list sizes
  every row from one it drew.
- **Toast**: `--ink` background, `--bg` text, 8 px, bottom centre.

## Copy

Plain and specific. Say what happened and what to do next. No exclamation
marks in success messages, no "Oops". On the website: no em dashes, no
buzzwords ("seamless", "elevate", "unleash"), and claim nothing the app
does not do today. The feature list is `CLAUDE.md` → Status, not a wish list.

## Website

- The hero shows the real app, from a screenshot of the packaged build,
  never a mock-up drawn in HTML.
- No stock photography or images from other hosts: the site makes no
  third-party requests, same as the app.
- Layout varies section to section (split hero, a two-column facts list, a
  plan comparison, a download block). No row of three identical cards.
- One label per intent: **Download** for getting the app, **Buy Base / Buy
  Pro** for paying, **Log in** for the account.

## References

Built by auditing the old interface against Taste Skill's redesign audit
(github.com/Leonxlnx/taste-skill) and Vercel's Web Interface Guidelines
(github.com/vercel-labs/web-interface-guidelines), with the DESIGN.md format
and the mail-client entries of github.com/VoltAgent/awesome-design-md as
reference for density and restraint. RATA borrows no other product's colours,
type or marks.
