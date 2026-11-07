import type { GameClient } from "./types";

const storageKey = "selectedClient";

/** "Retail (12.1.5)": the display name plus the version without its build number. */
export function clientLabel(client: GameClient): string {
  if (!client.version) return client.displayName;
  return `${client.displayName} (${client.version.split(".").slice(0, 3).join(".")})`;
}

/** The remembered client if it still exists, else Retail, else the first one. */
export function chooseClient(clients: readonly GameClient[], remembered: string | null): string | null {
  const exists = (folder: string | null) => clients.some((client) => client.folder === folder);
  if (exists(remembered)) return remembered;
  if (exists("_retail_")) return "_retail_";
  return clients[0]?.folder ?? null;
}

export function rememberedClient(): string | null {
  try {
    return localStorage.getItem(storageKey);
  } catch {
    return null;
  }
}

export function rememberClient(folder: string): void {
  try {
    localStorage.setItem(storageKey, folder);
  } catch {
    // Storage can be unavailable; the choice just isn't remembered then.
  }
}
