import { afterEach, describe, expect, it, vi } from "vitest"
import { api } from "./api"

class FakeApiTransport {
  readonly fetch = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => new Response(JSON.stringify({ checkout_id: "checkout-1", status: "PENDING" }), { status: 202 }))
}

describe("TaskLab API client", () => {
  afterEach(() => vi.unstubAllGlobals())

  it("sends only the commercial checkout intention with the session cookie", async () => {
    const transport = new FakeApiTransport()
    vi.stubGlobal("fetch", transport.fetch)
    await api("/checkouts", { method: "POST", headers: { "idempotency-key": "operation-1" }, body: JSON.stringify({ checkout_kind: "ON_DEMAND" }) })
    const [, request] = transport.fetch.mock.calls[0]
    expect(request?.credentials).toBe("include")
    expect(request?.headers).toMatchObject({ "idempotency-key": "operation-1" })
    expect(JSON.parse(String(request?.body))).toEqual({ checkout_kind: "ON_DEMAND" })
  })

  it("surfaces server failure text to the page", async () => {
    const transport = new FakeApiTransport()
    transport.fetch.mockResolvedValue(new Response(JSON.stringify({ message: "saldo insuficiente" }), { status: 409 }))
    vi.stubGlobal("fetch", transport.fetch)
    await expect(api("/executions", { method: "POST", body: "{}" })).rejects.toThrow("saldo insuficiente")
  })
})
