import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const workspaceId = "00000000-0000-4000-8000-000000000001";
const collectionId = "00000000-0000-4000-8000-000000000002";
const productId = "00000000-0000-4000-8000-000000000003";
const itemId = "00000000-0000-4000-8000-000000000005";
const priceId = "00000000-0000-4000-8000-000000000006";

test("operator follows a pending collection from the overview to its evidence", async ({
  page,
}) => {
  await page.route("**/v1/admin/billing/operations", async (route) =>
    route.fulfill({
      json: {
        pending_collections: 1,
        webhook_failures: 0,
        unprocessed_webhooks: 0,
        open_unmatched_payments: 0,
        outbox_backlog: 0,
        outbox_dead_letters: 0,
      },
    }),
  );
  await page.route("**/v1/admin/billing/records/collections?*", async (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: collectionId,
            kind: "collections",
            workspace_id: workspaceId,
            status: "SCHEDULED",
            occurred_at: "2026-09-24T12:00:00Z",
            collection_request_id: collectionId,
            collection_attempt_id: null,
            billing_payment_id: null,
            correlation_id: collectionId,
            provider_event_id: null,
            provider_payment_id: null,
            event_type: "INITIAL",
            amount_minor: 1000,
            currency: "BRL",
            failure_code: null,
            detail: "tx-1",
          },
        ],
        next_cursor: null,
      },
    }),
  );
  await page.route(
    `**/v1/admin/billing/records/collections/${collectionId}`,
    async (route) =>
      route.fulfill({
        json: {
          id: collectionId,
          kind: "collections",
          workspace_id: workspaceId,
          status: "SCHEDULED",
          occurred_at: "2026-09-24T12:00:00Z",
          collection_request_id: collectionId,
          collection_attempt_id: null,
          billing_payment_id: null,
          correlation_id: collectionId,
          provider_event_id: null,
          provider_payment_id: null,
          event_type: "INITIAL",
          amount_minor: 1000,
          currency: "BRL",
          failure_code: null,
          detail: "tx-1",
        },
      }),
  );

  await page.goto("/");
  await expect(page.getByText("Cobranças pendentes")).toBeVisible();
  await page
    .getByRole("link", { name: /Solicitações aguardando conclusão/ })
    .click();
  await expect(page).toHaveURL(/status=ACTIVE/);
  await page.getByRole("link", { name: /00000000…0002/ }).click();
  await expect(page.getByText("tx-1")).toBeVisible();
  await expect(page.getByRole("link", { name: workspaceId })).toHaveAttribute(
    "href",
    `/workspaces/${workspaceId}`,
  );
});

test("operator creates and checks a catalog product", async ({ page }) => {
  let submittedUsageModel = "";
  let submittedItemName = "";
  let published = false;
  let itemStatus = "INACTIVE";
  let productStatus = "INACTIVE";
  await page.route("**/v1/admin/catalog/products?*", async (route) =>
    route.fulfill({ json: { items: [], next_cursor: null } }),
  );
  await page.route("**/v1/products", async (route) => {
    if (route.request().method() === "POST") {
      const requestBody = route.request().postDataJSON() as {
        usage_model: string;
      };
      submittedUsageModel = requestBody.usage_model;
      return route.fulfill({ status: 201, json: productResponse() });
    }
    if (route.request().method() === "PATCH") {
      productStatus = "ACTIVE";
      return route.fulfill({
        json: { ...productResponse(), status: productStatus, version: 2 },
      });
    }
    await route.fulfill({
      json: { ...productResponse(), status: productStatus },
    });
  });
  await page.route(`**/v1/products/${productId}`, async (route) => {
    if (route.request().method() === "PATCH") productStatus = "ACTIVE";
    await route.fulfill({
      json: {
        ...productResponse(),
        status: productStatus,
        version: productStatus === "ACTIVE" ? 2 : 1,
      },
    });
  });
  await page.route(`**/v1/products/${productId}/items`, async (route) => {
    const item = route.request().postDataJSON() as { name: string };
    submittedItemName = item.name;
    await route.fulfill({ status: 201, json: itemResponse() });
  });
  await page.route(`**/v1/items/${itemId}/price-versions`, async (route) =>
    route.fulfill({ status: 201, json: priceResponse() }),
  );
  await page.route("**/v1/price-versions/*/publish", async (route) => {
    published = true;
    await route.fulfill({ json: { ...priceResponse(), state: "ACTIVE" } });
  });
  await page.route(`**/v1/items/${itemId}`, async (route) => {
    if (route.request().method() === "PATCH") itemStatus = "ACTIVE";
    await route.fulfill({
      json: {
        ...itemResponse(),
        status: itemStatus,
        version: itemStatus === "ACTIVE" ? 2 : 1,
      },
    });
  });
  await page.route("**/v1/admin/catalog/items?*", async (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: itemId,
            kind: "items",
            parent_id: productId,
            name: "Requests",
            status: itemStatus,
            created_at: "2026-09-24T12:00:00Z",
          },
        ],
        next_cursor: null,
      },
    }),
  );
  await page.route("**/v1/admin/catalog/prices?*", async (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: priceId,
            kind: "prices",
            parent_id: itemId,
            name: "unit",
            status: published ? "ACTIVE" : "DRAFT",
            created_at: "2026-09-24T12:00:00Z",
          },
        ],
        next_cursor: null,
      },
    }),
  );
  await page.route(`**/v1/price-versions/${priceId}`, async (route) =>
    route.fulfill({
      json: { ...priceResponse(), state: published ? "ACTIVE" : "DRAFT" },
    }),
  );
  await page.goto("/catalog/products");
  await page.getByRole("button", { name: "Criar" }).click();
  const formAccessibility = await new AxeBuilder({ page }).analyze();
  expect(
    formAccessibility.violations.filter((violation) =>
      ["critical", "serious"].includes(violation.impact ?? ""),
    ),
  ).toEqual([]);
  await page.getByRole("textbox", { name: "Nome" }).fill("Test Product");
  await page.getByRole("textbox", { name: "Créditos por cobrança" }).fill("5");
  await page.getByRole("button", { name: "Revisar e publicar" }).click();
  await page.getByRole("button", { name: "Confirmar publicação" }).click();
  await expect(page).toHaveURL(`/catalog/products/${productId}`);
  await expect(
    page.getByRole("heading", { name: "Test Product" }),
  ).toBeVisible();
  await expect(page.getByText("ACTIVE").first()).toBeVisible();
  expect(submittedUsageModel).toBe("CREDIT_METERED");
  expect(submittedItemName).toBe("Test Product");
  expect(published).toBe(true);
});

function productResponse() {
  return {
    product_id: productId,
    name: "Test Product",
    description: null,
    usage_model: "CREDIT_METERED",
    status: "INACTIVE",
    version: 1,
    created_at: "2026-09-24T12:00:00Z",
    updated_at: "2026-09-24T12:00:00Z",
  };
}

function itemResponse() {
  return {
    item_id: itemId,
    product_id: productId,
    parent_item_id: null,
    name: "Requests",
    unit_name: "unidade",
    quantity_scale: "1",
    status: "INACTIVE",
    version: 1,
    created_at: "2026-09-24T12:00:00Z",
    updated_at: "2026-09-24T12:00:00Z",
  };
}

function priceResponse() {
  return {
    price_version_id: priceId,
    item_id: itemId,
    pricing_model: "unit",
    unit_block_size: "1",
    credit_units: "5",
    effective_from: "2026-09-24T12:00:00Z",
    effective_until: null,
    accumulation_cycle: null,
    tiers: [],
    state: "DRAFT",
    version: 1,
    created_at: "2026-09-24T12:00:00Z",
  };
}

test("operator saves a product without items and can continue its setup", async ({
  page,
}) => {
  await page.route("**/v1/admin/catalog/products?*", async (route) =>
    route.fulfill({ json: { items: [], next_cursor: null } }),
  );
  await page.route("**/v1/products", async (route) =>
    route.fulfill({ status: 201, json: productResponse() }),
  );
  await page.route(`**/v1/products/${productId}`, async (route) =>
    route.fulfill({ json: productResponse() }),
  );
  await page.route("**/v1/admin/catalog/items?*", async (route) =>
    route.fulfill({ json: { items: [], next_cursor: null } }),
  );
  await page.route("**/v1/admin/catalog/products?*", async (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: productId,
            kind: "products",
            parent_id: null,
            name: "Test Product",
            status: "INACTIVE",
            created_at: "2026-09-24T12:00:00Z",
          },
        ],
        next_cursor: null,
      },
    }),
  );

  await page.goto("/catalog/products");
  await page.getByRole("button", { name: "Criar" }).click();
  await page.getByRole("textbox", { name: "Nome" }).fill("Test Product");
  await page.getByRole("button", { name: "Remover" }).click();
  await page.getByRole("button", { name: "Salvar sem publicar" }).click();

  await expect(page).toHaveURL(`/catalog/products/${productId}`);
  await expect(
    page.getByRole("button", { name: "Adicionar item de consumo" }),
  ).toHaveAttribute("href", `/catalog/items/new?product_id=${productId}`);
  await page.getByRole("button", { name: "Adicionar item de consumo" }).click();
  await expect(page.getByRole("combobox", { name: "Produto" })).toHaveValue(
    "Test Product · 00000000…0003",
  );
});

test("theme toggle applies and remembers the dark theme", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/catalog/products/new");
  await page.getByRole("button", { name: "Ativar modo escuro" }).click();
  await expect(page.locator("html")).toHaveClass(/dark/);
  await expect(
    page.getByRole("button", { name: "Ativar modo claro" }),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() => localStorage.getItem("subscription-admin-theme")),
    )
    .toBe("dark");

  await page.reload();
  await expect(page.locator("html")).toHaveClass(/dark/);
  await expect(
    page.getByRole("button", { name: "Ativar modo claro" }),
  ).toBeVisible();
});

test("theme follows system changes until the operator chooses a theme", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/catalog/products/new");
  await expect(page.locator("html")).not.toHaveClass(/dark/);

  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.locator("html")).toHaveClass(/dark/);
  await page.getByRole("button", { name: "Ativar modo claro" }).click();
  await expect(page.locator("html")).not.toHaveClass(/dark/);
  await expect
    .poll(() =>
      page.evaluate(() => localStorage.getItem("subscription-admin-theme")),
    )
    .toBe("light");

  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.locator("html")).not.toHaveClass(/dark/);
});

test("overview has no serious accessibility violations", async ({ page }) => {
  await page.route("**/v1/admin/billing/operations", async (route) =>
    route.fulfill({
      json: {
        pending_collections: 0,
        webhook_failures: 0,
        unprocessed_webhooks: 0,
        open_unmatched_payments: 0,
        outbox_backlog: 0,
        outbox_dead_letters: 0,
      },
    }),
  );
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Visão geral" }),
  ).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((violation) =>
      ["critical", "serious"].includes(violation.impact ?? ""),
    ),
  ).toEqual([]);
});

test("operator replays a quarantined inbox event after confirmation", async ({
  page,
}) => {
  const eventId = "00000000-0000-4000-8000-000000000004";
  let replayed = false;
  await page.route("**/v1/admin/integration-inbox?*", async (route) =>
    route.fulfill({
      json: {
        items: [
          {
            event_id: eventId,
            workspace_id: workspaceId,
            event_type: "workspace.updated",
            external_sequence: 7,
            processing_status: "QUARANTINED",
            correlation_id: collectionId,
            received_at: "2026-09-24T12:00:00Z",
            processed_at: null,
          },
        ],
        next_cursor: null,
      },
    }),
  );
  await page.route(
    `**/v1/admin/integration-inbox/${eventId}/replay`,
    async (route) => {
      replayed = true;
      await route.fulfill({
        json: {
          event_id: eventId,
          outcome: "applied",
          workspace_status: "ACTIVE",
          external_sequence: 7,
        },
      });
    },
  );
  await page.goto("/inbox");
  await page.getByRole("button", { name: "Repetir" }).click();
  await page.getByRole("button", { name: "Confirmar" }).click();
  await expect.poll(() => replayed).toBe(true);
});

test("audit and inbox pages have no serious accessibility violations", async ({
  page,
}) => {
  await page.route("**/v1/admin/audit-events?*", async (route) =>
    route.fulfill({ json: { items: [], next_cursor: null } }),
  );
  await page.route("**/v1/admin/integration-inbox?*", async (route) =>
    route.fulfill({ json: { items: [], next_cursor: null } }),
  );
  for (const [path, heading] of [
    ["/audit", "Auditoria"],
    ["/inbox", "Inbox"],
  ]) {
    await page.goto(path);
    await expect(page.getByRole("heading", { name: heading })).toBeVisible();
    const results = await new AxeBuilder({ page }).analyze();
    expect(
      results.violations.filter((violation) =>
        ["critical", "serious"].includes(violation.impact ?? ""),
      ),
    ).toEqual([]);
  }
});

test("operator reviews a direct credit and the panel sends an idempotency key", async ({
  page,
}) => {
  const lotId = "00000000-0000-4000-8000-000000000005";
  await page.route(
    `**/v1/workspaces/${workspaceId}/billing-config`,
    async (route) =>
      route.fulfill({
        json: {
          workspace_id: workspaceId,
          direct_credit_enabled: true,
          recurring_credit_enabled: true,
          version: 3,
          created_at: "2026-09-24T12:00:00Z",
          updated_at: "2026-09-24T12:00:00Z",
        },
      }),
  );
  await page.route(
    `**/v1/workspaces/${workspaceId}/credits/direct`,
    async (route) => {
      expect(route.request().headers()["idempotency-key"]).toMatch(
        /^[0-9a-f-]{36}$/i,
      );
      await route.fulfill({
        status: 201,
        json: {
          direct_credit_id: lotId,
          credit_lot_id: lotId,
          entry: { customer_wallet_entry_id: collectionId },
        },
      });
    },
  );
  await page.goto(`/workspaces/${workspaceId}/actions`);
  await page
    .getByRole("textbox", { name: "Transação ID" })
    .fill("support-credit-01");
  await page.getByRole("textbox", { name: "Unidades de crédito" }).fill("250");
  await page.getByRole("button", { name: "Revisar crédito" }).click();
  await expect(
    page.getByRole("heading", { name: "Conceder crédito?" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Confirmar crédito" }).click();
  await expect(
    page.getByText(new RegExp(`Crédito criado: lote ${lotId}`)),
  ).toBeVisible();
});
