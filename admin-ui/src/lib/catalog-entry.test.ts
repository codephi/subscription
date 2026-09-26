import { describe, expect, it } from "vitest";
import { entriesWithIds } from "./catalog-entry";

describe("entriesWithIds", () => {
  it("keeps duplicate record names distinguishable by shortened ID", () => {
    const entries = [
      {
        id: "00000000-0000-4000-8000-000000000001",
        kind: "products",
        name: "API",
        status: "ACTIVE",
        created_at: "2026-01-01T00:00:00Z",
      },
      {
        id: "00000000-0000-4000-8000-000000000002",
        kind: "products",
        name: "API",
        status: "ACTIVE",
        created_at: "2026-01-01T00:00:00Z",
      },
    ];

    expect(entriesWithIds(entries).map((entry) => entry.label)).toEqual([
      "API · 00000000…0001",
      "API · 00000000…0002",
    ]);
  });
});
