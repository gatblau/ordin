# SPEC: VisualDesign

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed; ADR-116, decision 98) · amended January 2026 (ADR-117, decision 99: V8 Icons; promoted to Approved by the user January 2026) · **Mode:** greenfield · **derivedFromHld:** 0.66.0
> **Implemented in:** the screens module `apps/desktop/ui/` — `src/styles/tokens.css` (every value below), the font file `apps/desktop/ui/src/assets/fonts/Jost-wght.ttf` with `apps/desktop/ui/src/assets/fonts/OFL.txt` (copies of `docs/logo/font/Jost[wght].ttf` and `docs/logo/font/OFL.txt`), the style blocks of its Svelte components, and the brand-token step of start-up (`src/lib/i18n/fluent.ts`, `brandTokens` and `applyBrandToDocument`); the default brand pack `crates/ordin-brand/build-brand/brand.json` (the brandable values); *from January 2026 (ADR-117)* the icon sprite `apps/desktop/ui/src/assets/icons/ordin-icons.svg` (the same bytes as the build pack's `crates/ordin-brand/build-brand/icons-light.svg`), the icon part `apps/desktop/ui/src/lib/icons/Icon.svelte` and its sprite step `apps/desktop/ui/src/lib/icons/sprite.ts`. Programs: D only.

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Applicability by program](#applicability-by-program)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Values](#values)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)

## Purpose

Fixes how the desktop's screens look — every colour in the light and dark themes, the typeface (Jost), the type sizes, the spacing, the corner radii, the panel widths and the drawing of the parts every screen shares — with the values of the prototype `docs/design/ordin-desktop/user-interface.html` (ADR-116, UX-A82), so the screens module draws what the prototype shows and a brand pack restyles the same roles. Where a prototype value fails WCAG 2.2 AA, this spec states the passing value that replaces it.

## Approach

All values live as CSS custom properties in one style sheet; components read only those properties (ADR-041: no literal colour in a component). Brandable properties take the active brand pack's token when present and this spec's value otherwise; fixed properties take this spec's value only. The arrangement of each screen and block follows the scene the Screens module names for it (ScreensModule.md, amendment *The look of the screens*). Rejected: *copying the prototype's style sheet whole* — it carries the prototype's own chrome and class names that do not map to the module's components; *screenshot comparison* — set aside by the user (ADR-116 item 6).

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | The screens module draws every screen with these values. | ADR-116 |
| Personal server (P) | No | It draws no screen; Telegram and Slack render in their own applications. | ADR-116; 12 §1 |
| Agent profile (A) | No | It draws no screen. | ADR-116 |

## Shared Context

- **Brand token names** (Brand.md, *Names chosen here*, amended January 2026): `^(color|font|space|radius)(\.[a-z0-9-]+){1,3}$`; colour tokens `color.<role>.<theme>` with `<theme>` ∈ {`light`, `dark`}; colour values `^#[0-9A-Fa-f]{6}$`; `space.*` and `radius.*` values `^[0-9]{1,3}(\.[0-9])?px$`; `font.*` values a family name of 1–64 characters from `[A-Za-z0-9 -]`. Required colour roles: `text`, `text-muted`, `background`, `surface`, `accent`, `on-accent`, `border-control`, `focus`. Optional colour roles (ADR-116 item 4): `text-secondary`, `accent-ink`, `accent-soft`, `chip`, `line`, `line-strong`, `primary`, `on-primary`. Refused prefixes: `color.danger.`, `color.waiting.`, `color.run-state.`, `color.approval.`.
- **`brand.get`** returns `design_tokens` as a map of token name to value, and `font_count`, the number of font files, for the active pack (ScreensModule.md, Shared Context); the event `brand_changed` announces a new active pack.
- **`brand.get_asset`** `{"asset": {"kind": "font", "index": i}}` returns `AssetBytes { media_type, base64 }` for font file `i` (0 ≤ i < `font_count`); media types `font/woff2`, `font/ttf`, `font/otf` (Brand.md, Internal Logic C2 and F). *From January 2026 (ADR-117):* `{"asset": {"kind": "icon_set_light"}}` and `{"kind": "icon_set_dark"}` return the active pack's icon sets, media type `image/svg+xml` or `image/png`, already checked by Brand (C2: ≤ 2 MiB, SHA-256, no script, `foreignObject`, event attribute, `javascript:` or external reference); `brand.get` reports `source` (`build` or `organisation_profile`).
- **Theme.** `display.appearance` is `light`, `dark` or absent; the screens set `data-theme` on `<html>` to `light` or `dark`, or remove it to follow the operating system (the function `applyAppearance` in `apps/desktop/ui/src/lib/app/bootstrap.ts`, which reads `settings.get` section `display`).
- **WCAG 2.2 contrast.** Relative luminance L = 0.2126 R + 0.7152 G + 0.0722 B with each sRGB channel linearised `c ≤ 0.04045 ? c/12.92 : ((c+0.055)/1.055)^2.4`; ratio = (L1 + 0.05) / (L2 + 0.05), lighter over darker. Text needs 4.5 (1.4.3); control borders, focus indicators and chart marks need 3.0 (1.4.11, 2.4.13).

## Public Interface

### P1. The custom property vocabulary

Every colour, font, size, spacing and radius a component uses is one of the properties in *Values*. A component's style block names no colour literal, no font family and no pixel value outside: borders of 1, 1.5, 2 or 2.5 pixels, the sizes of icons (11, 12, 13, 14, 16, 22 or 24 pixels; V8) and the layout values the *Values* tables list by component.

### P2. The brand-token mapping (`brandTokens(tokens) → Array<[property, value]>`)

| Token | Property set on `<html>` | Value written |
|---|---|---|
| `color.<role>.<theme>` (required or optional role) | `--brand-color-<role>-<theme>` | the token's value |
| `font.body` | `--brand-font-body` | the value in double quotes (`"Jost"`), except `system-ui`, written bare |
| `space.sm`, `space.md` | `--brand-space-sm`, `--brand-space-md` | the token's value |
| `radius.md`, `radius.lg` | `--brand-radius-md`, `--brand-radius-lg` | the token's value |
| any other token, or a name with a refused prefix | nothing | — (diagnostic `UI_BRAND_TOKEN_IGNORED`) |

`applyBrandToDocument(tokens)` first removes every `--brand-*` property it set before, then sets the pairs `brandTokens` returns, through `style.setProperty` on `document.documentElement`. CSSOM writes are allowed by the window's content security policy whatever nonces the build adds to `style-src`, whereas a generated `<style>` element would depend on `'unsafe-inline'`, which a nonce disables.

### P3. Theme scopes in `tokens.css`

Each brandable property is defined three times, in this order: on `:root` with the light value, inside `@media (prefers-color-scheme: dark) { :root:not([data-theme='light']) { … } }` with the dark value, and on `:root[data-theme='dark']` with the dark value. The light definition reads the brand's light token and the dark definitions read the dark token, each falling back as *Values* states, for example:

```css
:root { --color-line: var(--brand-color-line-light, var(--brand-color-border-control-light, #DDE2E8)); }
:root[data-theme='dark'] { --color-line: var(--brand-color-line-dark, var(--brand-color-border-control-dark, #2C3642)); }
```

Fixed properties are defined the same three times with literal values only.

### P4. Fonts

`tokens.css` declares the application's typeface once, so the first screen is drawn in it before `brand.get` answers:

```css
@font-face { font-family: "Jost"; src: url("../assets/fonts/Jost-wght.ttf") format("truetype"); font-weight: 100 900; font-style: normal; font-display: block; }
```

The file is loaded from the module's own bundle (`font-src 'self'`). A brand pack's font files are added at run time: for each `i` < `font_count`, `brand.get_asset` with `{kind: font, index: i}`, then `new FontFace(<font.body value>, <decoded bytes>, { weight: '100 900', style: 'normal' })`, `load()`, and `document.fonts.add(…)`. Every file of a pack is registered under the family its `font.body` token names *(chosen here: Brand carries no family per file; a pack supplies a variable font, or files that share one family)*. On `brand_changed`, the faces added for the previous pack are removed with `document.fonts.delete` before the new ones are added. Jost covers Latin and Cyrillic; other scripts fall through `--font-body` to the system font.

#### Example

Active pack tokens `{"color.accent.light":"#0B5FFF","color.accent.dark":"#6EA8FF","font.body":"Inter","color.danger.light":"#FF0000"}` → `brandTokens` returns `[["--brand-color-accent-light","#0B5FFF"],["--brand-color-accent-dark","#6EA8FF"],["--brand-font-body","\"Inter\""]]` and records one `UI_BRAND_TOKEN_IGNORED` for `color.danger.light` (Brand refuses such a pack first; this is the second line of defence). In the light theme `--color-accent` computes to `#0B5FFF`; `--color-danger` stays `#B42318`.

## Internal Logic

1. **At start** (ScreensModule.md, start-up step 1), after `brand.get` answers `ok`: `applyBrandToDocument(result.design_tokens)`. A failed `brand.get` leaves no `--brand-*` property, so every property takes this spec's value.
2. **On `brand_changed`:** `brand.get` again and step 1's call; the previous pack's properties are removed first (P2), so a role the new pack omits returns to its fallback.
3. **Fonts.** After step 1, the screens add the active pack's font files (P4). A file that fails to load or decode is skipped and recorded as `UI_BRAND_FONT_FAILED` in the diagnostics ring; text keeps the next family of `--font-body`.
4. **Theme.** No script chooses colours: the three scopes of P3 and `data-theme` decide. A theme change re-renders charts (ScreensModule.md step 8) because the canvas does not read custom properties after drawing.
5. **Fallback of an optional role.** An optional role absent from the active pack takes the pack's required role named in the *Fallback* column of the palette table; with no pack values at all, it takes this spec's value.
6. **Code editor and terminal.** Monaco takes two themes defined at first load, `ordin-light` and `ordin-dark` (base `vs` and `vs-dark`), whose `editor.background` is `--color-surface`, `editor.foreground` `--color-text`, and whose token rules colour `keyword`, `string`, `comment`, `number`, `type` and `function` with the six `--color-syntax-*` values of the theme; the terminal (xterm.js) takes `background` `--color-surface`, `foreground` `--color-text`, `cursor` `--color-accent`. Values are read with `getComputedStyle(document.documentElement)` when the editor or terminal is created and on each theme change.
7. **Review against the scene.** A change that adds or alters the drawing of a screen or block is accepted only after the screen, filled with data equivalent to the scene's, is set beside the scene named for it (ScreensModule.md amendment) at the same window width and theme, and every item of the checklist below holds; the reviewer records the scene id and the result in the change's description.
   - the same parts, in the same order and arrangement (rows, columns, groups, the place of each button);
   - the same palette role for every surface, border, text and mark;
   - the same type step (size and weight) for every text;
   - the same spacing step between and inside parts, and the same corner radius;
   - the same states: hover, pressed, selected, focused, disabled, folded.

8. **Icons** (*added January 2026, ADR-117*). At start, before the first screen is drawn, `installSprite()` adds one hidden `<svg aria-hidden="true" focusable="false" style="position:absolute;width:0;height:0">` as the first child of `<body>`, holding the 50 `<symbol>` elements of the module's own sprite (`ordin-icons.svg`, imported as text at build time). After step 1, when `brand.get` reports `source` other than `build`, the screens call `brand.get_asset` with `{kind: icon_set_light}` and `{kind: icon_set_dark}` and build, for each theme, a map of icon name → symbol:
   1. `media_type` `image/png` → that theme's map is empty; `UI_BRAND_ICONS_IGNORED` with detail `icon set <theme>: png`.
   2. `image/svg+xml` → decode the base64, parse with `DOMParser` as `image/svg+xml`; a parser error → empty map and `UI_BRAND_ICONS_IGNORED` with detail `icon set <theme>: unreadable`. Otherwise take each `symbol` element whose `id` is an icon name, keeping only these elements and attributes: `symbol` (`id`, `viewBox`), `path` (`d`, `stroke-width`), `circle` (`cx`, `cy`, `r`), `ellipse` (`cx`, `cy`, `rx`, `ry`), `rect` (`x`, `y`, `width`, `height`, `rx`, `ry`), `line` (`x1`, `y1`, `x2`, `y2`), `polyline` and `polygon` (`points`), `g` (none); any other element is dropped with its children. A map with no icon name → `UI_BRAND_ICONS_IGNORED` with detail `icon set <theme>: no named icons`.
   3. A failed `brand.get_asset` → empty map and `UI_BRAND_ICONS_IGNORED` with detail `icon set <theme>: unavailable`.
   Then, and again whenever the effective theme changes, each of the sprite's 50 symbols takes the map's symbol of the same name for the effective theme, or the module's own. On `brand_changed`, the maps are rebuilt (or emptied, for a build pack) and the sprite redrawn. The sprite is built with `document.createElementNS` and the copied attributes only; no markup string from a pack is ever assigned to `innerHTML`.

   *Example.* A pack's light icon set `<svg xmlns="http://www.w3.org/2000/svg"><symbol id="i-home" viewBox="0 0 16 16" class="x"><path d="M2 8l6-5 6 5" fill="red"/><image href="#a"/></symbol><symbol id="acme-logo"/></svg>` gives the map `{i-home → <symbol id="i-home" viewBox="0 0 16 16"><path d="M2 8l6-5 6 5"/></symbol>}`: `class` and `fill` are not on the list, `image` is dropped, `acme-logo` is not an icon name. The sprite's other 49 symbols stay ordin's.

## Values

### V1. Palette — brandable roles

| Role | Property | Brand token | Fallback (optional roles) | Light | Dark | Prototype | Used for |
|---|---|---|---|---|---|---|---|
| Ground | `--color-background` | `color.background.<t>` | — (required) | `#F2F4F7` | `#11161C` | `--ground` | the window, the open screen's area, the composer's text box, selected tab, folded block |
| Surface | `--color-surface` | `color.surface.<t>` | — (required) | `#FFFFFF` | `#1A2129` | `--surface` | side rail, header bars, tab rows, blocks, screen sections, dialogs, buttons, form fields |
| Text | `--color-text` | `color.text.<t>` | — (required) | `#17212B` | `#E8EDF2` | `--ink` | all body text and headings |
| Muted text | `--color-text-muted` | `color.text-muted.<t>` | — (required) | `#52606D` | `#9AA6B3` | `--muted` | secondary lines, rail section headings, hints, timestamps |
| Secondary text | `--color-text-secondary` | `color.text-secondary.<t>` | `text` | `#3D4A57` | `#C3CCD6` | `--ink-2` | unselected tab labels, supporting text that must stay strong |
| Accent | `--color-accent` | `color.accent.<t>` | — (required) | `#1F4FB8` | `#7FA6F5` | `--accent` | the selected tree row's outline, step numbers, the new-tab dot, drop outlines, `go` buttons' border |
| On accent | `--color-on-accent` | `color.on-accent.<t>` | — (required) | `#FFFFFF` | `#11161C` | `--on-accent` | text on an accent fill |
| Accent text | `--color-accent-ink` | `color.accent-ink.<t>` | `text` | `#163A88` | `#A9C3FA` | `--accent-ink` | link buttons, `go` button text, the *talking to* band, pressed button text |
| Accent soft | `--color-accent-soft` | `color.accent-soft.<t>` | `surface` | `#E3EAF7` | `#22314A` | `--accent-soft` | selected tree row, pressed buttons, the *talking to* band, the person's own messages |
| Chip | `--color-chip` | `color.chip.<t>` | `surface` | `#EEF2F8` | `#24303D` | `--chip` | chips, neutral state labels, tab hover |
| Divider | `--color-line` | `color.line.<t>` | `border-control` | `#DDE2E8` | `#2C3642` | `--line` | borders of blocks, sections, rail, tab rows, bubbles, dividers |
| Strong outline | `--color-line-strong` | `color.line-strong.<t>` | `border-control` | `#A9B4C1` | `#4A5868` | `--line-strong` | decorative outlines (lifecycle steps, type markers, the narrow conversation's edge), the empty run-state dot |
| Control border | `--color-border-control` | `color.border-control.<t>` | — (required) | `#7D8A98` | `#647384` | none (UI-D114) | borders of buttons, text boxes and selects (not of rail items, *Needs you* items and tabs, V6) |
| Focus | `--color-focus` | `color.focus.<t>` | — (required) | `#1F4FB8` | `#7FA6F5` | `--accent` | the focus outline: 3 px solid, offset 2 px |
| Primary | `--color-primary` | `color.primary.<t>` | `text` | `#17212B` | `#E8EDF2` | `--primary-bg` | the primary button's fill, the logo tile |
| On primary | `--color-on-primary` | `color.on-primary.<t>` | `background` | `#FFFFFF` | `#11161C` | `--primary-fg` | text on the primary fill |

`<t>` is `light` for the light value and `dark` for the dark value.

### V2. Palette — fixed by the application (ADR-041, ADR-116 item 5)

| Property | Light | Dark | Prototype | Used for |
|---|---|---|---|---|
| `--color-waiting` | `#B4530A` | `#F0A35E` | `--need` | count badge fill, the waiting dot |
| `--color-waiting-text` | `#8A3F07` | `#F3B47A` | `--need-ink` | waiting state label text, *Needs you* item text |
| `--color-waiting-soft` | `#FFF8F1` | `#2A2119` | `--need-soft` | waiting state label and *Needs you* item fill, pressed microphone |
| `--color-waiting-line` | `#F0C9A3` | `#5C4127` | `--need-line` | *Needs you* item border, loop marks |
| `--color-on-status` | `#FFFFFF` | `#11161C` | none (UI-D115) | count badge text; the glyphs of run-state dots |
| `--color-done` | `#0E7C66` | `#4CC3A5` | `--done` | finished run-state dot |
| `--color-done-text` | `#0E6B58` | `#6FD3B9` | `--done-ink` | finished state label text |
| `--color-done-soft` | `#E6F4EF` | `#173229` | `--done-soft` | finished state label fill |
| `--color-danger` | `#B42318` | `#F28B82` | `--bad` | failed state label text, `danger` button border and text, field errors |
| `--color-danger-soft` | `#FDEDEC` | `#3A1F23` | `--bad-soft` | failed state label fill |
| `--color-approval-background` | `#FFFDF6` | `#1E2530` | `--app-bg` | approval card fill |
| `--color-approval-line` | `#1F4FB8` | `#7FA6F5` | `--app-line` | approval card border, the *from ordin* mark |
| `--color-diff-added` | `#E6F4EF` | `#173229` | `--add` | added lines and words |
| `--color-diff-removed` | `#FBEBEC` | `#3A1F23` | `--del` | removed lines and words |
| `--chart-1` … `--chart-6` | `#1F4FB8` `#C4501B` `#0E7C66` `#7A3FB0` `#9A6B00` `#57606A` | `#7FA6F5` `#F2A37A` `#4CC3A5` `#C9A3F0` `#E8C468` `#AAB4BF` | `--ch-1` … `--ch-6` | chart series, in order |
| `--chart-7`, `--chart-8` | `#00778A` `#6B6B6B` | `#4FB8C9` `#A3A3A3` | none (*chosen here*: the prototype draws at most six series) | the seventh and eighth series |
| `--color-kind-sharepoint`, `-confluence`, `-github`, `-gitlab`, `-local` | `#0B6FC2` `#3B5BDB` `#57606A` `#B4541A` `#0E7C66` | `#6CB4F0` `#93A8F5` `#AAB4BF` `#F0A270` `#4CC3A5` | `--t-sp` `--t-cf` `--t-gh` `--t-gl` `--t-lc` | where-it-lives icons in the tree and documents |
| `--color-chip-sharepoint-fill`, `-text` | `#E8F1FB`, `#0B4F8A` | `--color-chip`, `--color-text-secondary` | `.chip.sp` | where-it-lives chip, SharePoint |
| `--color-chip-confluence-fill`, `-text` | `#E9EEFB`, `#1F3F99` | `--color-chip`, `--color-text-secondary` | `.chip.cf` | where-it-lives chip, Confluence |
| `--color-chip-github-fill`, `-text` | `#EEF0F2`, `#2A3139` | `--color-chip`, `--color-text-secondary` | `.chip.gh` | where-it-lives chip, GitHub |
| `--color-chip-gitlab-fill`, `-text` | `#FCEFE6`, `#8A3B0E` | `--color-chip`, `--color-text-secondary` | `.chip.gl` | where-it-lives chip, GitLab |
| `--color-chip-local-fill`, `-text` | `--color-done-soft`, `--color-done-text` | `--color-done-soft`, `--color-done-text` | `.chip.lc` | where-it-lives chip, this computer |
| `--color-kind-word`, `-sheet`, `-slides`, `-pdf`, `-image`, `-crew`, `-memory` | `#2B5CB8` `#1D7A46` `#C4501B` `#B42318` `#7A3FB0` `#8A3F07` `#9A6B00` | `#8FB0F2` `#6FD39A` `#F2A37A` `#F28B82` `#C9A3F0` `#F3B47A` `#E8C468` | `--t-word` … `--t-mem` | file-kind and node-kind icons |
| `--color-syntax-keyword`, `-string`, `-comment`, `-number`, `-type`, `-function` | `#7A2E9E` `#0B6B3A` `#5E6B78` `#9A4A00` `#1F4FB8` `#6B4E00` | `#D2A8FF` `#7EE0A6` `#8B98A6` `#F2B47A` `#8FB0F2` `#E8C468` | `--syn-k` … `--syn-f` | code highlighting (Internal Logic step 6) |
| `--color-diagram-<kind>-fill`, `--color-diagram-<kind>-stroke` for `<kind>` ∈ agent, ask, if, me, each, until, call, path | fills `#dbeafe` `#fef3c7` `#ede9fe` `#ffedd5` `#ccfbf1` `#dcfce7` `#e0e7ff` `#f1f5f9`; strokes `#2563eb` `#b45309` `#7c3aed` `#c2410c` `#0f766e` `#15803d` `#4338ca` `#64748b` | fills `#1e3a5f` `#4a3510` `#35245e` `#4a2410` `#0f3d3a` `#14391f` `#272a5c` `#1f2937`; strokes `#60a5fa` `#fbbf24` `#a78bfa` `#fb923c` `#2dd4bf` `#4ade80` `#818cf8` `#94a3b8` | `--dg-*` | the workflow and lifecycle diagrams (11 §5.20, UX-A40) |
| `--color-diagram-ink` | `#0f172a` | `#f1f5f9` | `--dg-ink` | text inside diagram shapes |
| `--color-scrim` | `rgba(10,15,20,.4)` | `rgba(10,15,20,.4)` | `.cdlgwrap` | the backdrop behind a modal dialog |

### V3. Type (fixed, except the family)

| Property | Value | Prototype |
|---|---|---|
| `--font-body` | `var(--brand-font-body, "Jost"), system-ui, -apple-system, "Segoe UI", sans-serif` | `--brand-font` (`"Jost",system-ui,-apple-system,"Segoe UI",sans-serif`) |
| `--font-mono` | `ui-monospace, "SF Mono", Menlo, Consolas, monospace` | `--mono` |

Every step uses the body's line height, 1.45, except *Code* (1.5). Headings (`h1` to `h3`) are weight 700; every other text is weight 400 unless its row in V6 states 500 or 600.

| Step | Size | Used for (prototype selector) |
|---|---|---|
| Display | 22 px | the first-start card's heading (`.setup h1`); a key figure's value (`.kfig .kv`, weight 650) |
| Document title | 20 px | a Markdown document's title (`.mdv h1`) |
| Block title | 19 px | a block's heading on its screen (`.spanel .bhead h3`) |
| Screen title | 18 px | a screen's title (`.screen h2`); the brand word in the rail (`.brand`, weight 700) |
| Section title | 17 px | the workspace header (`.wshead h2`); an approval card's heading (`.approval h3`) |
| Body | 16 px | `body`; conversation text; form fields; the search box (`.find input`); a document's second heading (`.mdv h2`) |
| Block heading | 15 px | a block's heading in the conversation (`.bhead h3`) |
| Sub-heading | 15 px | a heading inside a screen section (`.screen h3`); a folded approval card's heading (weight 500) |
| Control | 14 px | rail buttons, *Needs you* items, the *talking to* band |
| Small | 13 px | tabs (weight 600 when selected), hints, small buttons, header selects, rail section headings (upper case, letter spacing 0.04 em, `--color-text-muted`) |
| Code | 13 px | code and diffs, in `--font-mono`, line height 1.5 |
| Label | 12 px | state labels (weight 600), chips, the count badge, notes under an item |
| Marker | 11 px | upper-case markers such as *You are here* (weight 700, letter spacing 0.04 em); a block's type marker, in `--font-mono` |

Text scales with the operating system's text size (11 §10): sizes are in CSS pixels and the WebView applies the system's zoom.

### V4. Spacing and radius

| Property | Value | Brand token | Used for |
|---|---|---|---|
| `--space-02` | 2 px | — | between a label and its own small parts |
| `--space-04` | 4 px | — | tab gaps, icon to text in small controls |
| `--space-06` | 6 px | — | between items of a list, between chips |
| `--space-sm` | `var(--brand-space-sm, 8px)` | `space.sm` | between buttons, inside a control (`8px 10px` padding pattern) |
| `--space-10` | 10 px | — | inside a tree row, between a block's parts |
| `--space-12` | 12 px | — | inside the composer, between sections of a dialog |
| `--space-14` | 14 px | — | inside an approval card and a screen section |
| `--space-md` | `var(--brand-space-md, 16px)` | `space.md` | the rail's padding and gaps, a screen's padding at narrow widths |
| `--space-24` | 24 px | — | a screen's padding (top and bottom), the workspace header's side padding |
| `--space-32` | 32 px | — | a screen's side padding, the composer's side padding beside the screen area |
| `--radius-sm` | 4 px | — | a tab's close control |
| `--radius-xs` | 6 px | — | type markers, loop marks |
| `--radius-md` | `var(--brand-radius-md, 8px)` | `radius.md` | buttons, text fields, selects, rail items, tabs' top corners, *Needs you* items |
| `--radius-row` | 7 px | — | tree rows |
| `--radius-list` | 10 px | — | lists in the rail |
| `--radius-lg` | `var(--brand-radius-lg, 12px)` | `radius.lg` | blocks, approval cards, screen sections, the composer's text box, lifecycle steps |
| `--radius-bubble` | 14 px | — | conversation bubbles |
| `--radius-card` | 16 px | — | the first-start card |
| `--radius-pill` | 999 px | — | chips, state labels, the count badge, dots |

### V5. The window's regions

| Region | Values | Prototype |
|---|---|---|
| Shell | a grid of three columns: side rail, screen area (at least 0, takes the rest), conversation panel | `.app` |
| Side rail | default width 290 px (stored preference `ordin.view.panel.rail`, range 200–480, unchanged); fill `--color-surface`; right border 1 px `--color-line`; padding `--space-md`; gap `--space-md` between groups | `.rail` |
| Conversation panel | default width 420 px (stored preference `ordin.view.panel.conversation`, range 280–640, unchanged); left border 1 px `--color-line` | `.app`, `.chathead` |
| Panel edge (separator) | 10 px wide hit area; a 2 px line in `--color-accent` on hover, focus and drag; transparent otherwise | `.grip` |
| Workspace header | padding 10 px 24 px; fill `--color-surface`; bottom border 1 px `--color-line`; title in *Section title* | `.wshead` |
| Tab row | padding 6 px 16px 0; gap 2 px; fill `--color-surface`; bottom border 1 px `--color-line` | `.tabs` |
| Screen | padding 24 px 32 px (16 px at a window of 700 px or less); gap 16 px; content at most 860 px wide | `.screen` |
| Tabbed screen body | padding 20 px 28 px (16 px at 700 px or less); an editing page: 10 px 16 px | `.sbody` |
| Composer | fill `--color-surface`; top border 1 px `--color-line`; padding 10 px 12px 6 px in the conversation panel; text box: fill `--color-background`, border 1 px `--color-border-control`, radius `--radius-lg`, padding 10 px 12 px, at least 44 px high | `.composer` |
| Modal dialog | over `--color-scrim`; width at most 460 px; at most 86 % of the window's height; padding 16 px 18 px; gap 10 px; shadow `0 12px 40px rgba(0,0,0,.28)`; buttons at the end of the last row | `.cdlg` |

A stored panel width the person set is kept; only the default changes (from 280 and 380).

- **V5-SCR-1 The window never scrolls** *(amended January 2026, approved by the user)*. The shell is exactly the window's height: one row of at least 0 that takes the whole height (`grid-template-rows: minmax(0, 1fr)`). The side rail, the screen area and the conversation panel each scroll inside themselves when their content is taller; the window itself never scrolls, so the side panels always reach the bottom edge. `html` and `body` have `overflow: hidden`, so the pixel that rounding leaves on a scaled display (found on the Windows journeys, 781 against 782) cannot make the window scroll.

### V6. Shared parts

| Part | Values | Prototype |
|---|---|---|
| Button | border 1 px `--color-border-control`; fill `--color-surface`; radius `--radius-md`; padding 7 px 12 px; at least 36 px high; *Control* text; disabled at opacity 0.55 | `.btn` |
| Primary button | fill `--color-primary`; text `--color-on-primary`; border transparent | `.btn.primary` |
| Go button | border 2 px `--color-accent`; text `--color-accent-ink`, weight 600 | `.btn.go` |
| Danger button | border 2 px `--color-danger`; text `--color-danger`, weight 600; never the default button | `.btn.danger` |
| Small button | padding 3 px 8 px; at least 28 px high; *Small* text | `.btn.small` |
| Link button | no border, no fill; text `--color-accent-ink`, underlined | `.btn.link` |
| Pressed toggle | fill `--color-accent-soft`; border `--color-accent`; text `--color-accent-ink`, weight 600 | `.btn[aria-pressed=true]` |
| Text field, select | fill `--color-background`; border 1 px `--color-border-control` (UI-D114); radius `--radius-md`; padding 6 px 8 px; full width of its form column; its label 6 px below the row's top; a select in the workspace header: padding 5 px 8 px, *Small* text, at most 190 px wide | `.form input`, `.form select`, `.wshead select` |
| Focus outline | 3 px solid `--color-focus`, offset 2 px; on every focusable element | `:focus-visible` |
| Block | fill `--color-surface`; border 1 px `--color-line`; radius `--radius-lg`; padding 12 px 14 px; gap 10 px; at most 780 px wide in the conversation, at most 1,100 px wide on its screen; folded: padding 6 px 12 px, fill `--color-background`, title weight 500 in `--color-text-muted` followed by " · closed" | `.block`, `.block.folded` |
| Approval card | fill `--color-approval-background`; border 2 px `--color-approval-line`; radius `--radius-lg`; padding 14 px; gap 10 px; at most 780 px wide; the *from ordin* line in *Label* weight 700 `--color-accent-ink` after a 10 px square of `--color-approval-line` with radius 3 px | `.approval` |
| Screen section | fill `--color-surface`; border 1 px `--color-line`; radius `--radius-lg`; padding 14 px; gap 8 px | `.screen section` |
| Tab | *Small* text in `--color-text-secondary`; padding 5 px 4 px 6 px 10 px; radius 8 px 8 px 0 0; at most 230 px wide, label cut with an ellipsis; hover fill `--color-chip`; selected: fill `--color-background`, border 1 px `--color-line` (no bottom), text `--color-text` weight 600; a new tab's dot: 7 px `--color-accent`; close control 20 px square, radius `--radius-sm`, icon 11 px in `--color-text-muted` | `.tab`, `.tabx` |
| Tree row | at least 30 px high; padding 2 px 6 px 2 px (2 px + 14 px per level); radius `--radius-row`; gap 5 px; selected: fill `--color-accent-soft`, border 1 px `--color-accent`, weight 700 | `.trow` |
| Rail item | border 1 px `--color-line`; radius `--radius-md`; padding 8 px 10 px; at least 40 px high; pressed fill `--color-accent-soft` | `.railbtn` |
| *Needs you* item | fill `--color-waiting-soft`; border 1 px `--color-waiting-line`; text `--color-waiting-text`; radius `--radius-md`; padding 7 px 10 px; *Control* text; second line *Label* in `--color-text-muted`, weight 400 | `.needs button` |
| Count badge | fill `--color-waiting`; text `--color-on-status`; radius `--radius-pill`; padding 0 7 px; *Label* text | `.count` |
| State label | radius `--radius-pill`; padding 1 px 8 px; *Label* text; neutral: fill `--color-chip`, text `--color-text`; waiting: `--color-waiting-soft` / `--color-waiting-text`; finished: `--color-done-soft` / `--color-done-text`; failed: `--color-danger-soft` / `--color-danger`; always with its word and icon (11 §10) | `.st` |
| Chip | fill `--color-chip`; radius `--radius-pill`; padding 2 px 8 px; *Label* text weight 400 | `.chip` |
| Conversation bubble | fill `--color-surface`; border 1 px `--color-line`; radius `--radius-bubble`; padding 10 px 14 px; the person's own: aligned to the end, fill `--color-accent-soft`; a turn at most 780 px wide | `.bubble`, `.turn` |
| *Talking to* band | fill `--color-accent-soft`; text `--color-accent-ink`; padding 8 px 16 px; *Control* text | `.talking` |
| Rail section heading | *Small* text, upper case, letter spacing 0.04 em, `--color-text-muted` | `.rail h2` |
| Brand | logo tile 28 px square, radius `--radius-md`, fill `--color-primary`; the word in *Screen title* | `.brand`, `.logo` |
| Run-state dot | 14 px circle, glyph 10 px in `--color-on-status`; empty: `--color-line-strong`, no glyph; running now: `--color-accent`, glyph •; finished: `--color-done`, glyph ✓; waiting: `--color-waiting`, glyph !; refused or failed: `--color-danger`, glyph × | `.dot` |

**Borders that are not control borders.** Rail items, *Needs you* items and tabs keep the prototype's divider borders (`--color-line`, `--color-waiting-line`, and none for an unselected tab): each is identified by its words and fill, so WCAG 1.4.11 asks no 3:1 border of them (ADR-116 item 2).

### V7. Contrast of the default values

Every ratio computed with the formula in *Shared Context*, rounded down to two decimals. All pass.

| Foreground | Background | Threshold | Light | Dark |
|---|---|---|---|---|
| Text | Ground | 4.5 | 14.78 | 15.42 |
| Text | Surface | 4.5 | 16.29 | 13.78 |
| Muted text | Ground | 4.5 | 5.85 | 7.33 |
| Muted text | Surface | 4.5 | 6.45 | 6.55 |
| Secondary text | Ground | 4.5 | 8.23 | 11.19 |
| Secondary text | Surface | 4.5 | 9.06 | 9.99 |
| On accent | Accent | 4.5 | 7.32 | 7.50 |
| Accent | Ground | 3.0 | 6.64 | 7.50 |
| Accent text | Ground | 4.5 | 9.55 | 10.27 |
| Accent text | Surface | 4.5 | 10.52 | 9.17 |
| Accent text | Accent soft | 4.5 | 8.71 | 7.39 |
| Text | Accent soft | 4.5 | 13.48 | 11.09 |
| Text | Chip | 4.5 | 14.50 | 11.38 |
| Chip text (SharePoint, Confluence, GitHub, GitLab) | its chip fill | 4.5 | 7.36, 8.11, 11.51, 6.86 | 8.26 (secondary text on chip) |
| On status (dot glyph) | Accent, Done, Danger | 4.5 | 7.32, 5.12, 6.57 | 7.50, 8.35, 7.60 |
| Control border | Ground | 3.0 | 3.19 | 3.74 |
| Control border | Surface | 3.0 | 3.52 | 3.34 |
| Focus | Ground | 3.0 | 6.64 | 7.50 |
| On primary | Primary | 4.5 | 16.29 | 15.42 |
| Waiting text | Waiting soft | 4.5 | 7.13 | 8.73 |
| On status | Waiting | 4.5 | 5.02 | 8.74 |
| Waiting | Ground | 3.0 | 4.55 | 8.74 |
| Done text | Done soft | 4.5 | 5.68 | 7.66 |
| Done | Ground | 3.0 | 4.65 | 8.35 |
| Danger | Danger soft | 4.5 | 5.79 | 6.27 |
| Danger | Surface | 4.5 | 6.57 | 6.79 |
| Text | Approval background | 4.5 | 16.00 | 13.08 |
| Approval line | Approval background | 3.0 | 7.19 | 6.36 |
| Accent text | Approval background | 4.5 | 10.34 | 8.71 |
| Chart 1 … 8 | Surface | 3.0 | 7.32, 4.65, 5.12, 6.63, 4.69, 6.38, 5.24, 5.32 | 6.70, 7.95, 7.46, 7.71, 9.68, 7.72, 6.98, 6.43 |

### V8. Icons (ADR-117, UX-A83; *added January 2026*)

**The set.** 50 icons, each an SVG `<symbol>` with `viewBox="0 0 16 16"`, drawn with `path`, `rect` and `circle` elements, copied element for element from the sprite of `docs/design/ordin-desktop/user-interface.html` (the `<svg>` that follows the comment "Icon set for the side-rail tree"; the brand symbols `b-mark` and `b-word`, in a separate `<svg>`, are not icons and are not copied). The only presentation attribute copied is `stroke-width="2.6"` on the path of `i-more`, which the prototype's drawing carries so its dots show. The names, in this order, are the *icon names*:

`i-chev`, `i-ws`, `i-docs`, `i-sp`, `i-cf`, `i-gl`, `i-gh`, `i-lc`, `i-folder`, `i-word`, `i-sheet`, `i-slides`, `i-pdf`, `i-page`, `i-md`, `i-image`, `i-agent`, `i-remote`, `i-task`, `i-crew`, `i-keptcrew`, `i-run`, `i-mail`, `i-chart`, `i-memory`, `i-know`, `i-mic`, `i-more`, `i-plus`, `i-chat`, `i-open`, `i-send`, `i-publish`, `i-remove`, `i-bin`, `i-stop`, `i-play`, `i-edit`, `i-info`, `i-key`, `i-search`, `i-home`, `i-eye`, `i-project`, `i-code`, `i-history`, `i-copy`, `i-term`, `i-fold`, `i-unfold`.

*amended January 2026 (user decision; Approved by the user January 2026):* the editor's toolbar (ScreensModule.md SM-NS-4.6) adds nine icons after these, ordin's own drawings in the same style (16 × 16, stroke only): `i-heading`, `i-bold`, `i-italic`, `i-bullets`, `i-numbers`, `i-link`, `i-quote`, `i-undo`, `i-redo` (Table draws `i-sheet`). The module's sprite holds the 59; the build pack's icon sets carry the first 50, and an icon a pack omits keeps ordin's drawing. *Amended January 2026 (user decision; Approved by the user January 2026):* three action icons follow the editor's nine, ordin's own drawings in the same style, for secondary actions drawn as small icon buttons: `i-check` (Save, Done), `i-back` (Back) and `i-save` (Save logs for support…); the sprite then holds 62.

**Drawing.**

| Property | Value | Prototype |
|---|---|---|
| Fill, stroke | fill none; stroke `currentColor`; stroke width 1.6 (2.6 for `i-more`'s path); round caps and joins | `.ico`, `#i-more` |
| Default size | 16 × 16 px; never shrinks in a flex row | `.ico` |
| Small (`small`) | 13 × 13 px — icons inside chips, the *Earlier* button, the view switch, the where-it-lives marks on a tree row | `.ico.sm` |
| Twist (`twist`) | 12 × 12 px; turned 90° when its row is open, over 0.12 s; when reduced motion is asked, turned without the animation | `.twist .ico` |
| Mark (`mark`) | 12 × 12 px — *Asks you first*, *Ordin checks the result* | `.wfmark .ico` |
| Needs-you tab (`needs-tab`) | 14 × 14 px — the *Needs you* tab of the conversation | `.chathead .needtab .ico` |
| Close (`close`) | 11 × 11 px — a screen tab's or conversation tab's **Close**, **Hide the conversation** | `.tabx .ico` |
| Document heading (`heading`) | 24 × 24 px, centred in a 44 px tile of `--color-chip` with radius 10 px | `.d1ico` |
| Icon tile colour | an icon before a row's or card's words takes `--color-text-secondary`, except: on a one-line block card and an agent row, `--color-accent-ink`; a running item `--color-accent`; a where-it-lives icon its `--color-kind-<origin>`; a file-kind icon its `--color-kind-<kind>`, with `i-page` in `--color-kind-confluence` and `i-md` untinted; *Knowledge*, *Memory* and its rows `--color-kind-memory`; a crew `--color-kind-crew`; a dashboard `--color-kind-sheet` (V2) | `.tico`, `.lcard .tico`, `.t-*` (screen name amended January 2026 (user decision; Approved by the user January 2026): Memory, ScreensModule.md SM-SF-25) |
| Gap to words | 4 px in small controls and marks (`--space-04`), the row's own gap elsewhere | `.wfmark`, `.trow` |

**Accessibility.** Every icon is `<svg class="icon icon-<size>" aria-hidden="true" focusable="false"><use href="#i-<name>"/></svg>` (`<size>` one of `default`, `small`, `twist`, `mark`, `needs-tab`, `close`, `heading`): hidden from assistive technology, never in the tab order. An icon never carries meaning alone; the words beside it do. A button whose only content is an icon exists only on a tree row (its actions and **More actions**, ⋯), as a document screen's **More actions** (⋯), as a screen tab's or conversation tab's **Close**, **Hide the conversation** and **Dictate**, and (*amended January 2026 (user decision; Approved by the user January 2026)*) as the ten buttons of the editor's toolbar (ScreensModule.md SM-NS-4.6), each with its shortcut in its `title`, and as a fact line's secondary actions (ScreensModule.md SM-SF-13: Details, Correct, History, Keep it, Remember everywhere, Forget, Open the page) in the same toolbar style (amended January 2026; user decision; Approved by the user January 2026), and as the buttons that hide and show the side panel and the conversation (ScreensModule.md SM-PC-1) (amended January 2026; user decision; Approved by the user January 2026); each is a focusable `<button>` with a non-empty `aria-label` (its verb) and a `title` (its tooltip) from the catalogue messages ScreensModule.md amendment *Icons* names.

**Status glyphs are not icons.** The prototype draws the glyphs of run states and checks as text, not from the sprite: the run-state dot's glyphs (• ✓ ! ×, V6 *Run-state dot*), the problems and tests line's ✓ and ⚠ (and ? for problems not checked), and the **Outside the company** mark's ↗ (`user-interface.html`, `outMark`). They stay text glyphs in `aria-hidden` spans beside their words, with the values V6 and their screens give; they satisfy ScreensModule.md's requirements that a run state, a problems line and the **Outside the company** mark carry an icon.

**Direction.** Under `[dir="rtl"]`, `i-chev`, `i-send`, `i-open` and `i-history` are mirrored (`transform: scaleX(-1)`); an open twist is turned 90° and not mirrored.

**Replacement by a brand pack** (Internal Logic step 8). The sprite in the document holds exactly the 50 names (59, amended January 2026 (user decision; Approved by the user January 2026): the editor's nine follow; 62, amended January 2026 (user decision; Approved by the user January 2026): the three action icons follow them). For each name, the drawing is the active pack's `<symbol>` of that name when the pack's icon set for the current theme is SVG and carries it, and ordin's own otherwise.

**Where icons are drawn.** The reference is the prototype's script: its `ico(…)` calls, the `icon:` of each tree node and action, and its maps `KICO`, `ORIGIN_ICO`, `FEEDICO`, `KIND_ICO` and `ptIco`. The tables below transcribe them. An icon is drawn where its screen part exists today; a part the screens do not draw yet gets its icon in the amendment that adds it, from these tables.

*File kinds* (`KIND_ICO`): Word `i-word`, spreadsheet `i-sheet`, slides `i-slides`, PDF `i-pdf`, page `i-page`, Markdown `i-md`, image `i-image`. *Where documents live* (`ORIGIN_ICO`): SharePoint `i-sp`, Confluence `i-cf`, GitHub `i-gh`, GitLab `i-gl`, this computer `i-lc`.

| Place | Icon | Scene |
|---|---|---|
| Side rail: **Home**, **Search or go to…**, **Collapse all** / **Expand all**, **New workspace**, the bin row | `i-home`, `i-search`, `i-fold` / `i-unfold`, `i-plus`, `i-bin` | `home`, `tree` |
| Side rail: the **Show** switch, Documents / Code | `i-docs` / `i-code` (Small) | `eng` |
| Tree: the twist of a row that opens | `i-chev` (Twist) | `tree` |
| Tree: a workspace; its groups Documents, Agents, Workflows, Dashboards, Email, Telegram, Slack, Running now, Knowledge, Memory, Earlier conversations | `i-ws`; `i-docs`, `i-agent`, `i-crew`, `i-chart`, `i-mail`, `i-chat`, `i-chat`, `i-run`, `i-know`, `i-memory`, `i-history` | `tree` (screen name amended January 2026 (user decision; Approved by the user January 2026): Memory, ScreensModule.md SM-SF-25) |
| Tree: **Hosted agents**; a connected server's group | `i-remote` | `hostedOps`, `srvTree` |
| Tree: where documents live, a folder; a document; a dashboard | the origin's icon (tinted), `i-folder`; the file kind's icon (tinted); `i-chart` | `tree`, `docs` |
| Tree and lists: an agent, an agent that answers elsewhere, a task, a crew, a kept crew or workflow, a running item | `i-agent`, `i-remote`, `i-task`, `i-crew`, `i-keptcrew`, `i-run` | `tree`, `library`, `runs` |
| Tree row or a document's screen: **More actions** | `i-more` | `tree`, `docs`, `md` |
| Workspace header: **Conversation**; **Earlier**; **Team knowledge** | `i-chat`; `i-history` (Small); `i-know` (Small) | `tree`, `long`, `srvTree` |
| Screen tab: its kind; **Close** | by the screen: a block's screen as its one-line card (below); Earlier conversations `i-history`, a terminal `i-term`, code `i-code`, a review `i-edit`, a workflow `i-keptcrew`, a Markdown document `i-md`, the agent library `i-agent`, an agent's page `i-memory`, *Add to this workspace* `i-plus`, an earlier conversation `i-chat`, an agent draft `i-agent`, any other `i-docs`; `i-remove` (Close) | `switch` |
| Conversation: **Hide the conversation**; a conversation tab's **Close**; the *Needs you* tab; **Dictate** | `i-remove` (Close); `i-remove` (Close); `i-info` (Needs-you tab); `i-mic` | `long`, `needsTab` |
| One-line block card (ScreensModule step 6 placement) | `documents` `i-docs`; `preview` `i-word`; `document` `i-md`; `changes` `i-edit`; `plan_progress` `i-run`; `result` `i-task`; `memory` `i-memory`; `knowledge` `i-know`; `agent_library` `i-agent`; an approval `i-info`; every other kind `i-docs` | `docs`, `progress`, `report` |
| Home: a feed row — done, crew, changed, out of date, from Slack, anything else | `i-task`, `i-crew`, `i-docs`, `i-know`, `i-remote`, `i-task` | `home` |
| Search or go to…: a result — a workspace, an agent (or one that answers elsewhere), a workflow, a dashboard, a knowledge page, an earlier conversation, a document; the actions Home, New workspace, Settings, Add documents, Summarise, Clear, Earlier conversations | `i-ws`, `i-agent` (`i-remote`), `i-keptcrew`, `i-chart`, `i-know`, `i-history`, its file kind's icon, else `i-word`; `i-home`, `i-plus`, `i-key`, `i-plus`, `i-fold`, `i-chat`, `i-history` | `findAll` |
| *Add to this workspace*: a choice — documents by where they live, code, email, Telegram, Slack, Jira | the origin's icon; `i-code`; `i-mail`; `i-chat`; `i-chat`; `i-task` | `newWs`, `addCode` |
| Lists: a *Needs you* row; an earlier conversation; an email message; an email link choice (folder or address); a Telegram or Slack message or chat; a Jira issue; a binned workspace; a code repository | `i-info`; `i-chat`; `i-mail`; `i-folder` or `i-mail`; `i-chat`; `i-task`; `i-ws`; its host's origin icon | `needsTab`, `mailRead`, `tgRead`, `skChannel`, `addCode` |
| Memory: an entry from an agent, or the person's own page | `i-agent`, `i-page` (both in `--color-kind-memory`) | `memory` (screen name amended January 2026 (user decision; Approved by the user January 2026): Memory, ScreensModule.md SM-SF-25) |
| Parts of an agent repository: a part by kind — workspace profile, guideline, rule or operating mode, memory, context resource, tool binding, journey, any other | `i-agent`, `i-md`, `i-key`, `i-memory`, `i-docs`, `i-code`, `i-run`, `i-task` (`ptIco`) | `partsBiz`, `partRead` |
| Handover: a resource handed over | its file kind's icon (`i-sheet`, `i-pdf`, else `i-word`) | `hoScreen` |
| Chips: an attachment; a copy kept of a document; a knowledge page; a lifecycle stage | `i-sheet`, `i-pdf`, else `i-word` (Small); its file kind's icon, else `i-page` (Small); `i-know` (Small); `i-run` (Small) | `mailRead`, `md`, `know`, `hoScreen` |
| A voice note in a Telegram or Slack message | `i-mic` (Small) | `tgRead` |
| Marks: *Asks you first*, *Ordin checks the result* | `i-info`, `i-eye` (Mark) | `flow` |
| Document heading (a document's screen) | its file kind's icon (Document heading) | `md` |
| A shared workflow's origin line | the origin's icon | `flow` |

*Actions*, in a tree row's actions, its **More actions** menu, a screen's menu or an icon-with-words button. The icon goes with the action's words as the prototype's `acts` entries name it; an action listed here keeps its icon wherever it appears, and an action not listed has none.

| Action | Icon |
|---|---|
| Ask Ordin about …, Talk to … | `i-chat` |
| Add documents, Browse the company's agent library, Import workflow…, Import as an agent…, Import as a part… | `i-plus` |
| Search documents, Workflows in linked repositories…, Search pages | `i-search` |
| Open (a workflow, dashboard, email link, Telegram or Slack link), Show progress, Read it | `i-eye` |
| Open (a Markdown document) | `i-md` |
| Open in [application], Export…, Open (a hosted agent) | `i-open` |
| Open in editor, Add a code repository | `i-code` |
| Open terminal | `i-term` |
| Review changes, Show the lifecycle | `i-eye` |
| Show history | `i-history` |
| Show as a document | `i-md` |
| Check the code against the documents | `i-chat` |
| Copy path | `i-copy` |
| Share knowledge with a team…, Share Ordin's memory and knowledge pages with your team | `i-publish` |
| Connect again… | `i-plus` |
| Server settings | `i-info` |
| Open Knowledge | `i-know` |
| Open earlier conversations, Earlier conversations | `i-history` |
| Show where it lives | `i-folder` |
| What it will and won't do | `i-info` |
| What it's made of | `i-task` |
| Run …, Refresh, a task's start (`[task]…`) | `i-play` |
| Stop crew…, Stop — what's written so far is kept, Stop sharing with the team | `i-stop` |
| Change, Rename… | `i-edit` |
| Duplicate… | `i-copy` |
| Hand over…, Send back… | `i-send` |
| Follow a stage…, Change the stage it follows… | `i-run` |
| Close finished items | `i-fold` |
| Who can use it | `i-key` |
| Publish to Confluence… | `i-publish` |
| Remove … (from this workspace, a workspace, an agent, a workflow, a dashboard, a link, a code repository), Remove from workspace…, Move to Bin…, Disconnect this computer…, Remove this server | `i-remove` |

## Data Model

None. No table, file or stored value is added; the two panel-width preferences keep their keys and ranges (ScreensModule.md, Data Model).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A font file of the active pack fails to load or decode (P4) | internal (diagnostics ring only; the file is skipped) | `UI_BRAND_FONT_FAILED` | `{ at, code: "UI_BRAND_FONT_FAILED", backend, detail: "font <index>" }` |
| A token of the active pack has a name P2 does not map, or a refused prefix | internal (recorded in the diagnostics ring only; the token is not applied) | `UI_BRAND_TOKEN_IGNORED` | `{ at, code: "UI_BRAND_TOKEN_IGNORED", backend, detail: "<token name>" }` — the name only, never the value |
| `brand.get` fails at start or after `brand_changed` | the reply's `ServiceError` is shown by ScreensModule step 11 (`ErrorLine`); every property keeps or returns to this spec's value | (the reply's code) | — |
| An optional role is absent from the active pack | not an error: the fallback of V1 applies | — | — |
| The active pack's icon set for a theme is a PNG, cannot be parsed, carries no icon name, or cannot be fetched (Internal Logic step 8) | internal (diagnostics ring only; ordin's own icons are drawn) | `UI_BRAND_ICONS_IGNORED` | `{ at, code: "UI_BRAND_ICONS_IGNORED", backend, detail: "icon set <theme>: png" \| "…: unreadable" \| "…: no named icons" \| "…: unavailable" }` |
| A company pack's icon set carries some icon names only | not an error: each omitted name keeps ordin's drawing (V8) | — | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: The look of the screens (ADR-116)

  Scenario: Edge case — a screen taller than the window (V5-SCR-1)
    Given a screen whose content is twice the window's height
    When it is open
    Then the screen area scrolls and the window does not
    And the side rail and the conversation panel reach the bottom of the window

  Scenario: Happy path — the default values in the light theme
    Given the active brand pack is ordin's own build pack
    And display.appearance is "light"
    When the window shows Home
    Then the computed background-color of <body> is rgb(242, 244, 247)
    And the computed background-color of the side rail is rgb(255, 255, 255)
    And the computed color of <body> is rgb(23, 33, 43)
    And the computed font-size of <body> is 16px and its line-height is 23.2px
    And the first family of the computed font-family of <body> is "Jost"
    And document.fonts.check("16px Jost") is true

  Scenario: Happy path — the default values in the dark theme
    Given the active brand pack is ordin's own build pack
    And display.appearance is "dark"
    When the window shows Home
    Then the computed background-color of <body> is rgb(17, 22, 28)
    And the computed background-color of the side rail is rgb(26, 33, 41)
    And the computed color of <body> is rgb(232, 237, 242)

  Scenario: Happy path — a brand token reaches the screens
    Given an active pack whose color.accent.light is "#0B5FFF"
    And display.appearance is "light"
    When the window shows a screen with a Go button
    Then the computed border-color of the Go button is rgb(11, 95, 255)

  Scenario: Edge — an optional role falls back to the pack's required role
    Given an active pack with color.border-control.light "#767676" and no color.line.light
    When the window shows a block in the light theme
    Then the computed border-color of the block is rgb(118, 118, 118)

  Scenario: Edge — a new pack removes the previous pack's tokens
    Given an active pack with color.chip.light "#ABCDEF"
    When brand_changed announces a pack without color.chip.light and with color.surface.light "#FAFAFA"
    Then the computed background-color of a chip in the light theme is rgb(250, 250, 250)

  Scenario: Error — a brand font file cannot be decoded
    Given an active pack with font.body "Acme Sans" and one font file whose bytes are not a font
    When the screens add the pack's fonts
    Then the diagnostics ring holds UI_BRAND_FONT_FAILED with detail "font 0"
    And text is drawn in the next family of --font-body

  Scenario: Error — a fixed colour in the tokens is ignored
    Given brand.get returns design_tokens with color.danger.light "#FF0000"
    When the screens apply the tokens
    Then the computed color of a failed state label is rgb(180, 35, 24)
    And the diagnostics ring holds UI_BRAND_TOKEN_IGNORED with detail "color.danger.light"

  Scenario: Error — brand.get fails at start
    Given brand.get answers with an error
    When the window shows Home in the light theme
    Then the computed background-color of <body> is rgb(242, 244, 247)

  Scenario: Edge — the count badge in the dark theme
    Given display.appearance is "dark" and one item waits in Needs you
    When the window shows the side rail
    Then the computed color of the count badge is rgb(17, 22, 28)
    And its computed background-color is rgb(240, 163, 94)

  Scenario: Edge — a control border passes contrast
    Given display.appearance is "light"
    When the window shows a Button
    Then the computed border-color of the Button is rgb(125, 138, 152)

  Scenario: Edge — a stored panel width is kept
    Given ordin.view.panel.rail is stored as 260
    When the window opens
    Then the side rail is 260 pixels wide
    And with no stored value it is 290 pixels wide
  Scenario: Happy path — ordin's icons are in the document (ADR-117)
    Given the active brand pack is ordin's own build pack
    When the window shows Home
    Then the document holds one hidden sprite with exactly the 50 icon names of V8
    And no request other than the IPC channel was made for it

  Scenario: Happy path — an icon is decoration
    Given the window shows the side rail
    Then the Home button holds an svg with aria-hidden "true" and focusable "false" that uses "#i-home"
    And the Home button's accessible name is its words

  Scenario: Edge — a company pack replaces one icon
    Given an active company pack whose light icon set is an SVG with one symbol "i-home"
    And display.appearance is "light"
    When the screens apply the pack
    Then the sprite's "i-home" is the pack's drawing
    And each of the other 49 symbols is ordin's drawing

  Scenario: Edge — the icon follows the text colour
    Given display.appearance is "dark"
    When the window shows the Home button
    Then the computed stroke of its icon equals the computed color of the button

  Scenario: Edge — direction-bearing icons mirror
    Given the document's dir is "rtl"
    When the window shows a closed tree row and a Send button
    Then the computed transform of the twist's icon and of i-send is a mirror on the x axis

  Scenario: Error — a PNG icon set
    Given an active company pack whose light icon set has media type "image/png"
    When the screens apply the pack in the light theme
    Then every symbol of the sprite is ordin's drawing
    And the diagnostics ring holds UI_BRAND_ICONS_IGNORED with detail "icon set light: png"

  Scenario: Error — a symbol with an element outside the allowed list
    Given an active company pack whose light icon set's "i-home" holds a path and an image element
    When the screens apply the pack in the light theme
    Then the sprite's "i-home" holds the path and no image element

  Scenario: Edge — an icon-only button has a name and a tooltip
    Given the window shows a screen tab
    Then its Close button has a non-empty accessible name and a non-empty title

  Scenario: Error — an icon set that cannot be parsed
    Given an active company pack whose light icon set has media type "image/svg+xml" and bytes that are not XML
    When the screens apply the pack in the light theme
    Then every symbol of the sprite is ordin's drawing
    And the diagnostics ring holds UI_BRAND_ICONS_IGNORED with detail "icon set light: unreadable"

  Scenario: Error — an icon set with no icon name
    Given an active company pack whose light icon set is an SVG with one symbol "acme-logo"
    When the screens apply the pack in the light theme
    Then every symbol of the sprite is ordin's drawing
    And the diagnostics ring holds UI_BRAND_ICONS_IGNORED with detail "icon set light: no named icons"
```

Tests (vitest, in `apps/desktop/ui/tests/`): one per scenario above. Computed values are obtained with a resolver written in the test support code that reads `src/styles/tokens.css`, applies the scope of the theme under test and the `--brand-*` properties `brandTokens` returns, and resolves `var()` with its fallbacks (*chosen here*: happy-dom's `getComputedStyle` does not resolve nested `var()` fallbacks dependably); the font scenarios stub `FontFace` and `document.fonts`; one table-driven test of `brandTokens` over every row of P2; one test that every brandable property of V1 has a fallback chain ending in its light and dark values; one test computing every pair of V7 from `tokens.css`'s literal values with the formula of *Shared Context*. A lint step fails the build when a component's style block contains a `#` colour, `rgb(`, `hsl(` or a `font-family` other than the two properties of V3 (the diagram, chart and code colours are read from properties too).

## Performance, Security, Observability

- **Performance targets:** applying a brand pack's tokens ≤ 5 ms for 500 tokens (Brand's limit); no additional request at start beyond `brand.get`, which already runs, for ordin's own pack (it carries no font file); a company pack adds one `brand.get_asset` per font file, at most 8; first paint unchanged (ScreensModule.md: ≤ 1 s after the window is created).
- **Security:** tokens are written only as custom property values through CSSOM, never into a style sheet's text; a value is used only where its role is read, so a token cannot add a rule or a selector; the window's content security policy is unchanged.
- **Icons (ADR-117):** ordin's sprite adds about 12 KB to the bundle and no request; a company pack adds two `brand.get_asset` calls (light and dark), each ≤ 2 MiB (Brand's limit); building and swapping the sprite ≤ 20 ms. A pack's SVG reaches the document only as the elements and attributes Internal Logic step 8 lists, created through the DOM, after Brand's own checks (no script, `foreignObject`, event attribute, `javascript:` or external reference); the content security policy is unchanged.
- **Observability:** `UI_BRAND_TOKEN_IGNORED`, `UI_BRAND_FONT_FAILED` and `UI_BRAND_ICONS_IGNORED` in the diagnostics ring (Settings › Diagnostics › This window), carrying the token's name or the font's index only.
- **Licence:** Jost is under the SIL Open Font License 1.1 (OSI-approved); `OFL.txt` ships beside the file in the module's bundle.

## Gaps

None.
