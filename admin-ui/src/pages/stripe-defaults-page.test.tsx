// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { StripeDefaultsPage } from "./stripe-defaults-page";

const { getDefaultStripeCredentials, updateDefaultStripeCredentials } = vi.hoisted(() => ({
  getDefaultStripeCredentials: vi.fn(),
  updateDefaultStripeCredentials: vi.fn(),
}));

vi.mock("@/api/billing-api", () => ({
  getDefaultStripeCredentials,
  updateDefaultStripeCredentials,
}));

beforeEach(() => {
  getDefaultStripeCredentials.mockReset().mockResolvedValue({
    configured: true,
    environment: "TEST",
    account_reference: "acct_demo",
    api_secret_configured: true,
    webhook_secret_configured: true,
    configuration_version: 4,
  });
  updateDefaultStripeCredentials.mockReset().mockResolvedValue({});
});

it("saves default credentials without exposing previously stored values", async () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <StripeDefaultsPage />
    </QueryClientProvider>,
  );

  expect(await screen.findByText(/Conta acct_demo/)).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Chave secreta"), {
    target: { value: "sk_test_replacement" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Salvar alterações" }));

  await waitFor(() => {
    expect(updateDefaultStripeCredentials.mock.calls[0][0]).toEqual({
      expected_version: 4,
      secret_key: "sk_test_replacement",
      webhook_secret: undefined,
    });
  });
  expect(await screen.findByRole("status")).toBeTruthy();
});
