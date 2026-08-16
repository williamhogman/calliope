## The shell

The two sidebars die. The new shell is edge chrome around a permanently clear map:

- **Lens strip (top center):** the 7 layers become icon lenses with hotkeys 1–7 and a compact overlay flyout (rivers, snow, routes, resources, labels, winds, hillshade). The contextual legend renders as a slim strip attached under the lens bar only when a lens needs one. World generation (seed/size) moves into a small "New world" dialog off the brand chip — it's a once-per-session action and doesn't deserve permanent chrome.
- **Inspector dock (bottom left):** one stable panel, content reflows to the selection — terrain cell, settlement, people, trade route, deposit, named feature, war, market good. Replaces today's hover box + detail panel + half the right sidebar.
- **Outliner (right rail, collapsible to a 44px rail):** tabs for Towns / Peoples / Markets / Chronicle. Virtualized rows, sort/filter, free-text filter, star-to-pin (pins float to top and survive filters). Click = select + fly-to; modifier-click = select without moving the camera.
- **Time cluster (bottom right):** pause, speed pips (1/3/12), date. Always on top, keyboard-first (Space, N, +/-).
- **Situations tray (top right):** one chip per *ongoing* phenomenon — active wars, mines nearing exhaustion, golden ages, famines. Chip expands to a mini status card with a jump-to link. This is where long-running chronicle arcs live instead of scrolling away in the feed.
- **Toasts (top center, under lens strip):** ambient one-liners for major moments (war declared, wonder raised, gold strike), click to jump. Everything else logs silently to the Chronicle.

## Interaction grammar (strict)

- **Hover** — ephemeral tooltip: cell readout, settlement chip stats, route goods/cost, deposit richness, label meaning. Gone on mouse-out.
- **Click** — select: populates the inspector, draws a selection halo on the map, no camera movement.
- **Double-click / Enter from search** — select + fly-to.
- **Right-click (long-press on touch)** — small context menu: fly here, pin to outliner, open in inspector, copy coordinates.

Unified picking: settlements, deposits, route polylines (segment distance), feature labels, and territory — one prioritized hit-test index, so *everything drawn is inspectable* (today only settlements are).

## Explainability — every number answers "why"

The signature Paradox feature, done with the known pitfalls fixed:

- Any underlined value in a tooltip/inspector expands **in place** into its contributing terms (indented ledger lines), max 2 levels deep, "+N more" collapses the small terms, breadcrumb row on top.
- Tooltips are **pinnable**: click the pin icon and it becomes a draggable card that survives other hovers — capped at 3 pinned cards.
- Backed by a real engine API (see technical section): price = base × scarcity × shock terms; settlement growth = base rate × food × trade × era terms; site attractiveness = fertility/coast/river/delta/ore terms. The UI never fakes a formula the sim didn't report.

## Entity depth (what the inspector shows)

- **Settlement:** pop history sparkline, tier/culture/ruler, food & growth (explainable), goods with chief export, trade partners listed *per route* with carried goods, cost, and sail fraction (data already in the payload, never surfaced), recent events involving it.
- **People/culture:** era timeline with tech acquisition dates, polity, ruler line, treasury/lore, wars (clickable), settlement list, culture-colored territory flash on hover.
- **Market good:** price history sparkline (ring buffer per tick), base vs now, trend, top producing settlements, related discoveries/depletions from the chronicle.
- **Route:** endpoints, goods, cost breakdown land/sea/river, animated highlight on the map while inspected.
- **Deposit:** resource, richness, months left, working settlement, discovery event link.
- **War:** belligerents, start date, war name, related chronicle entries, front-line settlements.

## Chronicle & notifications

- Full history retained engine-side (today the client truncates to 200 events) with paged fetch; the Chronicle tab virtualizes thousands of entries.
- Filter chips by kind (myth, war, tech, discovery, …) and by entity; clicking an entry selects its subject and offers fly-to.
- Per-kind notification tier (log / toast) in a small settings popover, persisted.

## Semantic zoom & labels

Discrete zoom tiers — World, Region, Local — with hysteresis so labels never flicker at boundaries. Importance score (population, tier, feature size) decides survival in the existing greedy placement pass. Settlement markers grow data at closer tiers (name → name+pop chip → full banner), Civ-VI-style. Oceans/continents fade out zoomed in; straits, fords, marshes fade in — the current `_labelAlpha` curves formalized into tiers.

## Mobile

Same grammar, condensed chrome: bottom bar becomes Lenses / Search / Time / Almanac; inspector opens as a peek-height bottom sheet expandable to full; outliner lives inside Almanac; long-press = right-click. Pinned tooltips are desktop-only.

## Visual language

Keep the identity — deep navy, gold accent, Cinzel display, Inter text — and tighten the craft: `tabular-nums` on every numeric column, right-aligned, fixed precision; green/red strictly for deltas; a single-silhouette recolorable SVG icon set for the 10 goods + resource categories (tintable for scarce/surplus/depleted states, reused at 12–48px from tooltips to map markers); a whisper of cartographic ornament (hairline rules, small-caps section titles) instead of today's heavy glass cards. Panel chrome loses weight: thinner borders, less blur, more map.
