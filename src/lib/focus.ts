// Moving around the window with the keyboard alone.
//
// A row of items marked `data-roving` inside a `rovingGroup` (the activity
// bar, the tabs) takes one Tab stop: the arrow keys, Home and End move
// between its items. F6 and Shift+F6 jump between the window's parts, each
// marked `data-part`.

import type { KeyboardEvent as ReactKeyboardEvent } from "react";

const FOCUSABLE = 'button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

function items(group: Element): HTMLElement[] {
  return [...group.querySelectorAll<HTMLElement>("[data-roving]")].filter(
    (el) => !(el as HTMLButtonElement).disabled && el.offsetParent !== null,
  );
}

/** Keeps exactly one item of the group in the Tab order: the one last focused, else the chosen one. */
function settle(group: Element, focused?: HTMLElement) {
  const list = items(group);
  if (!list.length) return;
  const keep =
    (focused && list.includes(focused) && focused) ||
    list.find((el) => el.tabIndex === 0 && el === document.activeElement) ||
    list.find((el) => el.matches('[aria-current="page"], [aria-selected="true"]')) ||
    list[0];
  for (const el of list) el.tabIndex = el === keep ? 0 : -1;
}

/**
 * Makes the items marked `data-roving` inside an element a single Tab stop.
 * Pass it as the element's `ref`; it works for elements shown later too.
 */
export function rovingGroup(group: HTMLElement | null) {
  if (!group) return;
  settle(group);
  const onFocus = (e: FocusEvent) => {
    const el = e.target as HTMLElement;
    if (el.hasAttribute("data-roving")) settle(group, el);
  };
  // Items come and go (tabs open and close); re-pick after each change.
  const watch = new MutationObserver(() => settle(group, document.activeElement as HTMLElement));
  watch.observe(group, { childList: true, subtree: true, attributeFilter: ["aria-current", "aria-selected"] });
  group.addEventListener("focusin", onFocus);
  return () => {
    watch.disconnect();
    group.removeEventListener("focusin", onFocus);
  };
}

/** Arrow keys, Home and End for a `rovingGroup`. */
export function rovingKeys(e: ReactKeyboardEvent<HTMLElement>, orientation: "horizontal" | "vertical") {
  const target = e.target as HTMLElement;
  if (!target.hasAttribute("data-roving")) return;
  const list = items(e.currentTarget);
  const at = list.indexOf(target);
  if (at < 0) return;
  const [back, forward] = orientation === "horizontal" ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"];
  let next: number;
  if (e.key === forward) next = (at + 1) % list.length;
  else if (e.key === back) next = (at - 1 + list.length) % list.length;
  else if (e.key === "Home") next = 0;
  else if (e.key === "End") next = list.length - 1;
  else return;
  e.preventDefault();
  list[next].focus();
}

/** Moves focus to the next (1) or previous (-1) part of the window. */
export function focusPart(step: 1 | -1) {
  const parts = [...document.querySelectorAll<HTMLElement>("[data-part]")].filter((p) => p.offsetParent !== null);
  if (!parts.length) return;
  const at = parts.findIndex((p) => p.contains(document.activeElement));
  const part = parts[at < 0 ? (step === 1 ? 0 : parts.length - 1) : (at + step + parts.length) % parts.length];
  const target =
    part.querySelector<HTMLElement>('[data-roving][tabindex="0"]') ??
    [...part.querySelectorAll<HTMLElement>(FOCUSABLE)].find((el) => el.tabIndex >= 0 && el.offsetParent !== null);
  if (target) return target.focus();
  if (!part.hasAttribute("tabindex")) part.tabIndex = -1;
  part.focus();
}
