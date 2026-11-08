// Mirrors the JSON the Tauri commands return. The `json_matches_the_typescript_types` test in
// src-tauri/src/commands.rs fails when the Rust side changes shape.

export type WowRootSource = "env" | "settings" | "devSandbox";

export interface ResolvedWowRoot {
  path: string;
  source: WowRootSource;
}

export type GameType = "standard" | "forever" | "vanilla" | "tbc" | "wrath" | "cata" | "mists";

export interface GameClient {
  /** Folder name, e.g. `_retail_`; identifies the client in other commands. */
  folder: string;
  path: string;
  /** Product code, e.g. `wow_classic_era`. */
  product: string;
  displayName: string;
  /** Full version, e.g. `12.1.5.70783`. */
  version: string | null;
  /** TOC interface number, e.g. `120105`. */
  interface: number | null;
  gameType: GameType | null;
}

export interface InstalledAddon {
  /** Main folder name; identifies the addon within a client. */
  name: string;
  title: string;
  notes: string | null;
  version: string | null;
  author: string | null;
  interfaces: number[];
  /** Every folder of the addon, main folder first. */
  folders: string[];
  website: string | null;
  curseProjectId: string | null;
  wagoId: string | null;
  wowiId: string | null;
}

/** GitHub's API rate limit that release lookups count against. */
export interface RateLimit {
  limit: number;
  remaining: number;
  /** Seconds since the Unix epoch. */
  resetsAt: number;
}

export type ErrorKind =
  | "noWowRoot"
  | "wowRootMissing"
  | "unknownClient"
  | "config"
  | "io"
  | "network"
  | "rateLimited"
  | "badToken"
  | "credentials"
  | "source"
  | "internal";

/** What every command rejects with. */
export interface CommandError {
  kind: ErrorKind;
  message: string;
}
