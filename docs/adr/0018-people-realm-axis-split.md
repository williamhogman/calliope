# ADR-0018: People and Realm are separate axes

- **Status:** Accepted
- **Date:** 2026-08-17
- **Touches:** `game/rust/src/ids.rs` (`PeopleId`, `RealmId`),
  `game/rust/src/culture.rs` (`People`), `game/rust/src/politics.rs`
  (`Realm`), `game/rust/src/state.rs` (`Peoples` wall),
  `game/rust/src/society.rs`, `game/rust/src/chronicle.rs`, the wire
  (`snapshot.rs`, `pack.rs`), and the client's political/culture layers.

## Context

One `Culture` was simultaneously a *people* (tongue, gods, demonym, name
bank, arts) and a *state* (treasury, ruler, wars, opinion row, territory
kernel). Consequences, all observed in long runs:

- The only resolution for a rising was `culture::secede` — minting an
  entire new people for what is a political break. Peoples multiplied
  without bound; nothing ever subtracted.
- A conquered town changed its *people* the moment it changed hands, so
  the map carried no minorities, no assimilation pressure, no memory —
  the fuel the M11–M13 arc needs did not exist in the state.
- Realm count could only rise (secession) or fall (annexation-to-zero);
  coups, succession crises, unions and collapse-to-successors were
  unrepresentable.

## Decision

Two id spaces, two clocks:

- **`People`** (`culture.rs`, `PeopleId`) — changes on a generational
  clock: style, pantheon, demonym, name bank, lineage (`parent`), arts
  (`Society` stays keyed by people). Lore and tech travel with the
  people.
- **`Realm`** (`politics.rs`, `RealmId`) — changes on a political clock:
  name, ruling house, seat (capital settlement), treasury, founded
  month. All M4 state — opinion, AE dread, coalitions, wars, sieges,
  tribute, vassalage, asabiyyah, legitimacy — lives on the realm axis.
- Settlements carry **both** `people` and `realm`. Conquest and
  secession move `realm` only; `people` moves only by assimilation or
  merging (M12). `namer` remains a people (names carry tongues).
- Realms map N:1 onto peoples through their **crown** (`Realm.people`);
  one realm may hold towns of several peoples.
- At the dawn realms are created 1:1 with peoples. After that the axes
  drift apart: risings mint realms (never peoples), unions and
  collapses remove and create realms, divergence and merging (M12)
  move the people axis.
- Wire: the territory grid's owner is the **realm**; a parallel
  people-axis influence grid ships for the culture layer. Bootstrap
  ships both tables; the inspector reads "a town of the {people}, under
  the crown of {realm}".

## Consequences

- Minorities exist as state, not prose: `s.people != crown(s.realm)`
  is readable everywhere — the unrest ladder (M11), kinship drift
  (M12) and successor-state collapse (M13) become bookkeeping over it.
- `culture::secede` is deleted; `politics::realm_secede` replaces it
  and coins only a realm name and house in the parent people's tongue.
- Treasury moves from `Society` to `Realm`; tech/knowledge stay with
  the people. A people conquered whole keeps its arts.
- The wire grows: settlements ship two small ints instead of one;
  bootstrap ships a `realms` table; the pack gains a `peoples` grid.
- Physical layers are untouched — terrain through resources hash
  identically before and after the split.

## Alternatives considered

- **Realm as a view over culture groups** — no new id space, realms as
  partitions of cultures. Rejected: the N:1 crown mapping and mixed-
  people realms cannot be expressed, which is the entire point.
- **Keep one axis, add a "minority" flag per town** — a boolean cannot
  say *which* people a minority is, so kinship, assimilation and
  divergence have nothing to read.
