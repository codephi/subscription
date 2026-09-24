// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { expect, it, vi } from "vitest";
import { WorkspaceListPage } from "./workspace-list-page";

const workspaceId = "00000000-0000-4000-8000-000000000001";

vi.mock("@/api/client", () => ({
  getWorkspace: vi.fn(),
  listWorkspaces: vi.fn().mockResolvedValue({
    items: [
      {
          workspace_id: "00000000-0000-4000-8000-000000000001",
        operational_status: "ACTIVE",
        external_sequence: 2,
        updated_at: "2026-09-24T12:00:00Z",
      },
    ],
    next_cursor: null,
  }),
}));

it("lists workspace projections and requires a full UUID for direct lookup", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceListPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  expect(
    (await screen.findByRole("link", { name: workspaceId })).getAttribute(
      "href",
    ),
  ).toBe(`/workspaces/${workspaceId}`);
  expect(
    (screen.getByRole("button", { name: "Buscar" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
