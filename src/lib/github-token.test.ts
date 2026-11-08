import { describe, expect, it } from "vitest";
import { describeRateLimit } from "./github-token";

const utcTime = (date: Date) => date.toISOString().slice(11, 16);

describe("describeRateLimit", () => {
  it("says how many requests are left and when the limit resets", () => {
    const limit = { limit: 5000, remaining: 4987, resetsAt: 1_800_000_720 };
    expect(describeRateLimit(limit, utcTime)).toBe(
      "4,987 of 5,000 requests left this hour, resets at 08:12.",
    );
  });

  it("leaves out the reset while nothing is used", () => {
    const limit = { limit: 60, remaining: 60, resetsAt: 1_800_000_720 };
    expect(describeRateLimit(limit, utcTime)).toBe("60 of 60 requests left this hour.");
  });

  it("formats the time in the local time zone by default", () => {
    const limit = { limit: 60, remaining: 0, resetsAt: 1_800_000_720 };
    expect(describeRateLimit(limit)).toMatch(/^0 of 60 requests left this hour, resets at .*\d.*\.$/);
  });
});
