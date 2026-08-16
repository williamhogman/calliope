// Keyed list rendering (E8.2): thin wrappers over Solid's For/Index so the
// tagged-template UI keeps its shape. `each` keys rows on item identity —
// rows whose item survives a recompute keep their DOM, so a tick that
// patches three settlements re-renders three rows, not the whole rail.

import { For, Index } from "solid-js";

/** Keyed by item reference — for lists whose items persist across updates. */
export const each = (list, row) =>
  For({
    get each() { return list(); },
    children: row,
  });

/** Keyed by position — for short lists of primitives or churning rows. */
export const eachIdx = (list, row) =>
  Index({
    get each() { return list(); },
    children: row,
  });
