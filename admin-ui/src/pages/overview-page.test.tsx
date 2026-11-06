// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { OverviewPage } from "./overview-page";

vi.mock("@/api/billing-api", () => ({
  getOperations: vi.fn().mockResolvedValue({
    pending_collections: 2,
    webhook_failures: 0,
    unprocessed_webhooks: 1,
    open_unmatched_payments: 3,
    outbox_backlog: 4,
    outbox_dead_letters: 0,
  }),
}));

afterEach(() => vi.clearAllMocks());

it("shows operational counters and a path to accounts", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <OverviewPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  expect(await screen.findByText("Cobranças pendentes")).toBeTruthy();
  expect(screen.getByText("Pagamentos não conciliados")).toBeTruthy();
  expect(
    screen.getByRole("link", { name: /Abrir accounts/ }).getAttribute("href"),
  ).toBe("/accounts");
});
