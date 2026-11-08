import { describe, expect, it } from "vitest";
import { filterAddons } from "./addon-filter";
import type { InstalledAddon } from "./types";

function addon(name: string, fields: Partial<InstalledAddon> = {}): InstalledAddon {
  return {
    name,
    title: name,
    notes: null,
    version: null,
    author: null,
    interfaces: [],
    folders: [name],
    website: null,
    curseProjectId: null,
    wagoId: null,
    wowiId: null,
    ...fields,
  };
}

const addons = [
  addon("DBM-Core", {
    title: "<DBM Core> Main Core",
    author: "MysticalOS, QartemisT",
    folders: ["DBM-Core", "DBM-StatusBarTimers"],
  }),
  addon("LFGBulletinBoard", { title: "LFG Bulletin Board", author: "Vyscî, Bigsxy" }),
  addon("Questie", { title: "Questie v12.6.2", folders: ["Questie", "QuestieDB"] }),
];

const names = (query: string) => filterAddons(addons, query).map((a) => a.name);

describe("filterAddons", () => {
  it("keeps everything for a blank query", () => {
    expect(names("")).toEqual(["DBM-Core", "LFGBulletinBoard", "Questie"]);
    expect(names("   ")).toEqual(["DBM-Core", "LFGBulletinBoard", "Questie"]);
  });

  it("matches the title case-insensitively", () => {
    expect(names("main core")).toEqual(["DBM-Core"]);
    expect(names("BULLETIN")).toEqual(["LFGBulletinBoard"]);
  });

  it("matches the author, ignoring accents", () => {
    expect(names("qartemis")).toEqual(["DBM-Core"]);
    expect(names("vysci")).toEqual(["LFGBulletinBoard"]);
  });

  it("matches any folder name, not just the main one", () => {
    expect(names("statusbartimers")).toEqual(["DBM-Core"]);
    expect(names("questiedb")).toEqual(["Questie"]);
  });

  it("requires every word to match somewhere", () => {
    expect(names("dbm mystical")).toEqual(["DBM-Core"]);
    expect(names("dbm questie")).toEqual([]);
  });
});
