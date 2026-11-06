// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { clearPendingKey, pendingKey } from "./idempotency";

afterEach(() => localStorage.clear());

it("reuses the key for a retry and rotates it for another transaction", () => {
  const first = pendingKey("credit:one", "tx-1");
  expect(pendingKey("credit:one", "tx-1")).toBe(first);
  expect(pendingKey("credit:one", "tx-2")).not.toBe(first);
  clearPendingKey("credit:one");
  expect(localStorage.length).toBe(0);
});
