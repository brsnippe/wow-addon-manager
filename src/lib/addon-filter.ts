import type { InstalledAddon } from "./types";

/** Lowercase without diacritics, so "vysci" finds "Vyscî". */
function normalize(text: string): string {
  return text.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();
}

/**
 * Addons matching every word of `query` in their title, author or any folder name.
 * A blank query keeps everything.
 */
export function filterAddons(addons: readonly InstalledAddon[], query: string): InstalledAddon[] {
  const words = normalize(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...addons];
  return addons.filter((addon) => {
    const haystack = normalize([addon.title, addon.author ?? "", ...addon.folders].join("\n"));
    return words.every((word) => haystack.includes(word));
  });
}
