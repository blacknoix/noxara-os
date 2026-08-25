# 03-UIUX — Application shell & design system (Phase 1.3)

Status: **Active** for Phase 1.3. Specs deepen as modules land.

## Principles

1. **One product, not twelve.** Same shell, same `Table`, same filter grammar
   (`field operator value`), same detail layout everywhere.
2. **Sentence case.** Buttons are verbs (“Invite”, “Save view”).
3. **Permission-aware chrome.** Hide nav items the user cannot access; never
   show disabled dead links.
4. **Five designed states:** loading, empty, error (with `request_id`),
   permission-denied (with required permission), stale (as-of + refresh).
5. **WCAG 2.2 AA.** Focus rings, skip-to-content, labelled controls, honour
   `prefers-reduced-motion` / `prefers-color-scheme` / contrast themes.
6. **Keyboard-first.** Command bar ⌘K / Ctrl-K.
7. **Honest empties.** No fake CRM or finance metrics. Module widgets that are
   not built yet say so and disable or label enabling actions as coming.

## Shell

| Region | Spec |
|--------|------|
| Top bar | 56px — org switcher, command bar, create menu, notifications, help, avatar |
| Sidebar | Grouped Work / Sales / Finance / Ops / Insights / Settings; collapsed 64px icon mode (persisted) |
| Context panel | 380px collapsible — Phase 1.9 copilot placeholder |
| Breakpoints | Desktop ≥1280 preferred; tablet/mobile must not break (overlay sidebar &lt;1024; hide panel &lt;768) |

## Design system

Package: `@companyos/design-system` (upgrade in place). Gallery: `/dev/components`.

Core surfaces: inputs, **Table** (virtualised ≥200), **FilterBar**, Widget,
states, overlays (Modal focus-trap, Toast max 3), CommandBar, nav primitives.

Themes: `light`, `dark`, `high-contrast` via `data-theme`.

## Dashboard

Widget grid with per-role *layout* (Owner / Finance / Ops / Sales / Member).
Widgets return empty / setup / `module_not_enabled` payloads from
`GET /api/v1/dashboard`. Period selector present. First paint: skeleton, then
live empty widgets — do not block on a dozen APIs.

## Saved views

At least one collection (Members) uses Table + FilterBar with view state
round-tripped through the URL (`q`, `f`, `view`).

## Out of scope (later phases)

Real AI copilot (1.9), Document AI, CRM boards, invoice numbers, Storybook cloud,
pixel-perfect mobile apps.
