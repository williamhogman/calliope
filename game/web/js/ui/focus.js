// Focus discipline (E8.10): dialog popovers trap Tab and hand focus back
// to their opener on close; tablists rove a single tab stop with arrows.

import { onCleanup } from "solid-js";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * Attach via ref on a dialog root. Moves focus to the first control, keeps
 * Tab cycling inside, and restores focus to whatever opened the dialog
 * when it unmounts. Registered under the popover's reactive owner, so the
 * cleanup runs exactly when the conditional render drops the subtree.
 */
export function trapFocus(el) {
  const opener = document.activeElement;
  queueMicrotask(() => el.querySelector(FOCUSABLE)?.focus());
  const onKey = (e) => {
    if (e.key !== "Tab") return;
    const items = [...el.querySelectorAll(FOCUSABLE)];
    if (!items.length) return;
    const i = items.indexOf(document.activeElement);
    if (e.shiftKey && i <= 0) {
      e.preventDefault();
      items[items.length - 1].focus();
    } else if (!e.shiftKey && i === items.length - 1) {
      e.preventDefault();
      items[0].focus();
    }
  };
  el.addEventListener("keydown", onKey);
  onCleanup(() => {
    el.removeEventListener("keydown", onKey);
    if (opener?.isConnected) opener.focus();
  });
}

/**
 * Attach via ref on a role="tablist" container. One tab stop (the selected
 * tab); ArrowLeft/Right/Up/Down move and activate, Home/End jump.
 */
export function roveTabs(el) {
  const tabs = () => [...el.querySelectorAll('[role="tab"]')];
  const sync = () => {
    const list = tabs();
    const active =
      list.find((t) => t.getAttribute("aria-selected") === "true") || list[0];
    for (const t of list) t.tabIndex = t === active ? 0 : -1;
  };
  queueMicrotask(sync);
  const mo = new MutationObserver(sync);
  mo.observe(el, { attributes: true, subtree: true, attributeFilter: ["aria-selected"] });
  const onKey = (e) => {
    const list = tabs();
    const i = list.indexOf(document.activeElement);
    if (i < 0) return;
    let j = null;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") j = (i + 1) % list.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") j = (i - 1 + list.length) % list.length;
    else if (e.key === "Home") j = 0;
    else if (e.key === "End") j = list.length - 1;
    if (j == null) return;
    e.preventDefault();
    list[j].focus();
    list[j].click();
  };
  el.addEventListener("keydown", onKey);
  onCleanup(() => {
    mo.disconnect();
    el.removeEventListener("keydown", onKey);
  });
}
