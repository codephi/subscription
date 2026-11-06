// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, expect, it, vi } from "vitest";
import { AccountPage } from "./account-page";

const { reconcileProvisioning } = vi.hoisted(() => ({
  reconcileProvisioning: vi.fn(),
}));

vi.mock("@/api/client", () => ({
  getAccount: vi.fn().mockResolvedValue({
    account_id: "account-1",
    operational_status: "ACTIVE",
    external_sequence: 1,
    updated_at: "2026-09-24T12:00:00Z",
  }),
  getWallets: vi.fn().mockResolvedValue({
    account_id: "account-1",
    scope_version: "scope-1",
    ready: false,
    customer_wallet: { balance_credit_units: "0" },
    item_wallets: [],
  }),
  getProvisioning: vi.fn().mockResolvedValue({
    account_id: "account-1",
    scope_version: "scope-1",
    status: "PROVISIONING",
    expected_item_wallets: 1,
    materialized_item_wallets: 0,
  }),
  reconcileProvisioning,
}));

vi.mock("@/pages/account-panels", () => ({
  CreditsPanel: () => null,
  ItemUsagePanel: () => null,
  PlansPanel: () => null,
}));

beforeEach(() => {
  reconcileProvisioning.mockReset().mockResolvedValue({
    account_id: "account-1",
    scope_version: "scope-1",
    status: "ACTIVE",
    expected_item_wallets: 1,
    materialized_item_wallets: 1,
  });
});

it("provisions account wallets from the account detail", async () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/accounts/account-1"]}>
        <Routes>
          <Route path="/accounts/:accountId" element={<AccountPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );

  fireEvent.click(
    await screen.findByRole("button", { name: "Provisionar wallet" }),
  );

  await waitFor(() => {
    expect(reconcileProvisioning).toHaveBeenCalledWith("account-1");
  });
  expect(
    await screen.findByText("Provisionamento: ACTIVE (1/1 item wallets)."),
  ).toBeTruthy();
});
