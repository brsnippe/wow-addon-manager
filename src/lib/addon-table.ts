import type { InstalledAddon } from "./types";

export type SortKey = "name" | "status" | "latestVersion" | "gameVersion" | "authors";
export type SortDirection = "ascending" | "descending";

export interface Sort {
  key: SortKey;
  direction: SortDirection;
}

export const defaultSort: Sort = { key: "name", direction: "ascending" };

/** Clicking the sorted column flips its direction; another column starts ascending. */
export function nextSort(current: Sort, key: SortKey): Sort {
  if (current.key !== key) return { key, direction: "ascending" };
  return { key, direction: current.direction === "ascending" ? "descending" : "ascending" };
}

/** `120105` → `12.1.5`, `11509` → `1.15.9`. */
export function formatInterface(value: number): string {
  return `${Math.floor(value / 10000)}.${Math.floor(value / 100) % 100}.${value % 100}`;
}

/**
 * The interface number an addon targets on a client. Multi-flavor TOCs list several; the one
 * closest to the client's wins (the higher one on a tie), or the highest without a client.
 * Closest rather than same-major, because Classic Era (1.15) and Forever (1.60) share a major.
 */
export function targetInterface(addon: InstalledAddon, clientInterface: number | null): number | null {
  if (addon.interfaces.length === 0) return null;
  if (clientInterface === null) return Math.max(...addon.interfaces);
  const distance = (value: number) => Math.abs(value - clientInterface);
  return addon.interfaces.reduce((best, value) =>
    distance(value) < distance(best) || (distance(value) === distance(best) && value > best)
      ? value
      : best,
  );
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/**
 * Sorts a copy of `addons`. Missing values go last in either direction; ties fall back to the
 * title. Status and latest version aren't known until update checks exist, so those columns
 * currently sort by title.
 */
export function sortAddons(
  addons: readonly InstalledAddon[],
  sort: Sort,
  clientInterface: number | null,
): InstalledAddon[] {
  const sign = sort.direction === "ascending" ? 1 : -1;
  const byTitle = (a: InstalledAddon, b: InstalledAddon) =>
    collator.compare(a.title, b.title) || collator.compare(a.name, b.name);

  return [...addons].sort((a, b) => {
    switch (sort.key) {
      case "gameVersion":
        return (
          compareMissingLast(
            targetInterface(a, clientInterface),
            targetInterface(b, clientInterface),
            (x, y) => (x - y) * sign,
          ) || byTitle(a, b)
        );
      case "authors":
        return (
          compareMissingLast(a.author, b.author, (x, y) => collator.compare(x, y) * sign) ||
          byTitle(a, b)
        );
      default:
        return byTitle(a, b) * sign;
    }
  });
}

function compareMissingLast<T>(a: T | null, b: T | null, compare: (a: T, b: T) => number): number {
  if (a === null || a === "") return b === null || b === "" ? 0 : 1;
  if (b === null || b === "") return -1;
  return compare(a, b);
}
