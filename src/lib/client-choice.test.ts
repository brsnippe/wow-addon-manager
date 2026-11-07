import { describe, expect, it } from "vitest";
import { chooseClient, clientLabel } from "./client-choice";
import type { GameClient } from "./types";

function client(folder: string, displayName: string, version: string | null = null): GameClient {
  return {
    folder,
    path: `C:\\WoW\\${folder}`,
    product: folder,
    displayName,
    version,
    interface: null,
    gameType: null,
  };
}

describe("clientLabel", () => {
  it("shows the version without the build number", () => {
    expect(clientLabel(client("_retail_", "Retail", "12.1.5.70783"))).toBe("Retail (12.1.5)");
    expect(clientLabel(client("_ptr_", "PTR"))).toBe("PTR");
  });
});

describe("chooseClient", () => {
  const clients = [client("_classic_era_", "Classic Era"), client("_retail_", "Retail")];

  it("keeps the remembered client when it still exists", () => {
    expect(chooseClient(clients, "_classic_era_")).toBe("_classic_era_");
  });

  it("falls back to Retail, then to the first client", () => {
    expect(chooseClient(clients, "_ptr_")).toBe("_retail_");
    expect(chooseClient(clients, null)).toBe("_retail_");
    expect(chooseClient([clients[0]], null)).toBe("_classic_era_");
    expect(chooseClient([], "_retail_")).toBeNull();
  });
});
