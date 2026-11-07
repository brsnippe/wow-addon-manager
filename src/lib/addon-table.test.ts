import { describe, expect, it } from "vitest";
import {
  defaultSort,
  formatInterface,
  nextSort,
  sortAddons,
  targetInterface,
  type Sort,
} from "./addon-table";
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

const names = (addons: InstalledAddon[]) => addons.map((a) => a.name);

describe("formatInterface", () => {
  it("formats retail and classic interface numbers", () => {
    expect(formatInterface(120105)).toBe("12.1.5");
    expect(formatInterface(11509)).toBe("1.15.9");
    expect(formatInterface(16001)).toBe("1.60.1");
    expect(formatInterface(20505)).toBe("2.5.5");
  });
});

describe("targetInterface", () => {
  it("picks the interface closest to the client's", () => {
    const multi = addon("Multi", { interfaces: [11508, 11509, 16001, 20505, 120105] });
    expect(targetInterface(multi, 11509)).toBe(11509);
    expect(targetInterface(multi, 16001)).toBe(16001);
    expect(targetInterface(multi, 20506)).toBe(20505);
    expect(targetInterface(multi, 120105)).toBe(120105);
  });

  it("falls back to the nearest flavor when the client's isn't listed", () => {
    const tbcOnly = addon("TbcOnly", { interfaces: [20505, 50503, 120107] });
    expect(targetInterface(tbcOnly, 11509)).toBe(20505);
    expect(targetInterface(addon("Old", { interfaces: [100207, 110200] }), 120105)).toBe(110200);
  });

  it("prefers the higher interface on a tie, and the highest without a client", () => {
    expect(targetInterface(addon("Tie", { interfaces: [11500, 11510] }), 11505)).toBe(11510);
    expect(targetInterface(addon("Any", { interfaces: [110200, 100207] }), null)).toBe(110200);
    expect(targetInterface(addon("None"), 120105)).toBeNull();
  });
});

describe("nextSort", () => {
  it("toggles the current column and resets for a new one", () => {
    const descending = nextSort(defaultSort, "name");
    expect(descending).toEqual({ key: "name", direction: "descending" });
    expect(nextSort(descending, "name")).toEqual(defaultSort);
    expect(nextSort(descending, "authors")).toEqual({ key: "authors", direction: "ascending" });
  });
});

describe("sortAddons", () => {
  const addons = [
    addon("b", { title: "Bagnon", author: "Jaliborc", interfaces: [120000] }),
    addon("a", { title: "atlasLoot", author: null, interfaces: [120105] }),
    addon("d", { title: "Details 2", author: "Terciob", interfaces: [] }),
    addon("c", { title: "Details 10", author: "jaliborc", interfaces: [110200] }),
  ];
  const sorted = (sort: Sort) => names(sortAddons(addons, sort, 120105));

  it("sorts by title case-insensitively with natural numbers", () => {
    expect(sorted(defaultSort)).toEqual(["a", "b", "d", "c"]);
    expect(sorted({ key: "name", direction: "descending" })).toEqual(["c", "d", "b", "a"]);
  });

  it("sorts by game version with missing values last in both directions", () => {
    expect(sorted({ key: "gameVersion", direction: "ascending" })).toEqual(["c", "b", "a", "d"]);
    expect(sorted({ key: "gameVersion", direction: "descending" })).toEqual(["a", "b", "c", "d"]);
  });

  it("sorts by author and breaks ties by title", () => {
    expect(sorted({ key: "authors", direction: "ascending" })).toEqual(["b", "c", "d", "a"]);
    expect(sorted({ key: "authors", direction: "descending" })).toEqual(["d", "b", "c", "a"]);
  });

  it("does not modify the input", () => {
    const before = names(addons);
    sortAddons(addons, { key: "gameVersion", direction: "descending" }, 120105);
    expect(names(addons)).toEqual(before);
  });
});
