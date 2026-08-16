# ADR-0008: Vendored Solid.js UI without a build step

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/web/js/vendor/`, `game/web/js/ui/`, `game/web/index.html`

## Context

The map viewer's UI grew from imperative DOM code into panels, bottom
sheets, layer toggles, a chronicle feed and simulation controls — clearly
reactive-state territory. But the web app is deliberately buildless: static
files served as ES modules, no bundler, no node toolchain in the serve path.
The only build step in the whole project is `wasm-pack` for the engine.

## Decision

We use Solid.js for UI state and rendering, vendored as prebuilt ES modules
(`game/web/js/vendor/solid.js`, `store.js`, `html.js`, `web.js`) and driven
through the `html` tagged-template API instead of JSX. The app stays
buildless: any edit to `game/web/` is live on refresh.

## Consequences

- Fine-grained reactivity (signals/stores) without a compiler; UI code is
  plain modules loadable by any static server.
- No JSX means slightly noisier templates; acceptable at our UI size.
- Vendor upgrades are manual and deliberate — copied in, diffed, committed.
- The dev loop for UI is instant; only engine changes pay the wasm-pack tax.

## Alternatives considered

- **Keep imperative DOM code** — was already collapsing under panel/sheet
  state synchronization on mobile.
- **React via CDN** — heavier runtime, coarser update model for
  per-frame-adjacent UI (calendar scrub, hover inspector).
- **Adopt a bundler (Vite) for JSX** — reintroduces the toolchain the
  project structure exists to avoid; ruled out for the web layer.
