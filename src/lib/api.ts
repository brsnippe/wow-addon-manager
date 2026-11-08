import { invoke } from "@tauri-apps/api/core";
import type {
  CommandError,
  GameClient,
  InstalledAddon,
  RateLimit,
  ResolvedWowRoot,
} from "./types";

/** Calls a command and rejects with a `CommandError`, whatever went wrong. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toCommandError(error);
  }
}

/** Tauri itself rejects with plain strings (e.g. for bad arguments); wrap those too. */
export function toCommandError(error: unknown): CommandError {
  if (typeof error === "object" && error !== null && "kind" in error && "message" in error) {
    return error as CommandError;
  }
  return { kind: "internal", message: error instanceof Error ? error.message : String(error) };
}

export const wowRoot = () => call<ResolvedWowRoot | null>("wow_root");

export const listClients = () => call<GameClient[]>("list_clients");

/** Addons of the client in `folder` (e.g. `_retail_`), sorted by title. */
export const listAddons = (folder: string) => call<InstalledAddon[]>("list_addons", { folder });

export const githubTokenStored = () => call<boolean>("github_token_stored");

export const githubRateLimit = () => call<RateLimit>("github_rate_limit");

/** Checks the token with GitHub, then stores it. Rejects with `badToken` if GitHub refuses it. */
export const saveGithubToken = (token: string) => call<RateLimit>("save_github_token", { token });

export const removeGithubToken = () => call<void>("remove_github_token");
