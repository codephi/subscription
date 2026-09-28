// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { expect, it, vi } from "vitest";
import { WorkspaceListPage } from "./workspace-list-page";

const workspaceId = "00000000-0000-4000-8000-000000000001";

vi.mock("@/api/client", () => ({
  getWorkspace: vi.fn(),
  createWorkspace: vi.fn().mockResolvedValue({
    workspace_id: "00000000-0000-4000-8000-000000000002",
  }),
  terminateWorkspace: vi.fn().mockResolvedValue({
    workspace_status: "TERMINATED",
  }),
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

function CurrentPath() {
  return <output>{useLocation().pathname}</output>;
}

it("lists workspace projections and requires a full UUID for direct lookup", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceListPage />
        <CurrentPath />
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

it("creates a workspace and opens its detail page", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceListPage />
        <CurrentPath />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  fireEvent.change(screen.getByLabelText("Criado por"), {
    target: { value: "ops@example.com" },
  });
  fireEvent.click(
    screen.getAllByRole("button", { name: "Criar workspace" })[0],
  );
  await waitFor(() => {
    expect(
      screen.getByText("/workspaces/00000000-0000-4000-8000-000000000002"),
    ).toBeTruthy();
  });
});

it("requires confirmation before terminating a workspace", async () => {
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

  fireEvent.click((await screen.findAllByRole("button", { name: "Apagar" }))[0]);
  expect(
    await screen.findByText(/Planos, cobranças, carteiras e auditoria/),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Encerrar workspace" }));
  await waitFor(async () => {
    const api = await import("@/api/client");
    expect(api.terminateWorkspace).toHaveBeenCalledWith(workspaceId);
  });
});
