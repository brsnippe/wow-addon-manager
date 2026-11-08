import type { RateLimit } from "./types";

/** GitHub's page for creating a fine-grained token; its defaults (public repositories, read-only) are enough. */
export const NEW_TOKEN_URL = "https://github.com/settings/personal-access-tokens/new";

const count = new Intl.NumberFormat("en-US");

function localTime(date: Date): string {
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** E.g. `4,987 of 5,000 requests left this hour, resets at 14:32.` */
export function describeRateLimit(limit: RateLimit, formatTime = localTime): string {
  const left = `${count.format(limit.remaining)} of ${count.format(limit.limit)} requests left this hour`;
  if (limit.remaining >= limit.limit) return `${left}.`;
  return `${left}, resets at ${formatTime(new Date(limit.resetsAt * 1000))}.`;
}
