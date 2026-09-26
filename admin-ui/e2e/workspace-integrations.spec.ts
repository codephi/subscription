import { expect, test } from "@playwright/test";

const workspaceId = "00000000-0000-4000-8000-000000000001";
const connectionId = "00000000-0000-4000-8000-000000000009";

test("operator configures Stripe and resumes the webhook step", async ({ page }) => {
  let integration = integrationResponse("PENDING_SETUP", false);
  let sentApiKey = "";
  let sentWebhookSecret = "";
  await page.route("**/v1/admin/integrations/providers", (route) =>
    route.fulfill({
      json: [{ provider: "STRIPE", display_name: "Stripe", available: true }],
    }),
  );
  await page.route("**/v1/admin/workspaces/*/integrations", async (route) => {
    if (route.request().method() === "POST") {
      const body = route.request().postDataJSON() as { secret_key: string };
      sentApiKey = body.secret_key;
      await route.fulfill({ status: 201, json: integration });
      return;
    }
    await route.fulfill({ json: [integration] });
  });
  await page.route("**/v1/admin/workspaces/*/integrations/*", async (route) => {
    if (route.request().method() === "PATCH") {
      const body = route.request().postDataJSON() as { webhook_secret: string };
      sentWebhookSecret = body.webhook_secret;
      integration = integrationResponse("ACTIVE", true);
      await route.fulfill({ json: integration });
      return;
    }
    await route.fulfill({ json: integration });
  });
  await page.route("**/v1/admin/workspaces/*/integrations/*/test", (route) =>
    route.fulfill({
      json: {
        successful: true,
        account_reference: "acct_demo",
        environment: "TEST",
      },
    }),
  );

  await page.goto(`/workspaces/${workspaceId}/integrations`);
  await page.getByLabel("Chave secreta do Stripe").fill("sk_test_example_secret");
  await page.getByRole("button", { name: "Validar e continuar" }).click();
  await expect(page.getByText("Configuração pendente")).toBeVisible();
  expect(sentApiKey).toBe("sk_test_example_secret");

  await page.getByRole("button", { name: "Continuar configuração" }).click();
  await page.getByLabel("Segredo de assinatura (whsec_…)").fill("whsec_example_secret");
  await page.getByRole("button", { name: "Salvar e concluir" }).click();
  await expect(page.getByText("Integração pronta")).toBeVisible();
  expect(sentWebhookSecret).toBe("whsec_example_secret");

  await page.getByRole("button", { name: "Testar conexão" }).click();
  await expect(page.getByText("Conexão validada")).toBeVisible();
});

function integrationResponse(status: string, webhookConfigured: boolean) {
  return {
    billing_connection_id: connectionId,
    provider: "STRIPE",
    account_reference: "acct_demo",
    environment: "TEST",
    status,
    api_secret_configured: true,
    webhook_secret_configured: webhookConfigured,
    customer_reference: null,
    webhook_path: `/v1/billing/webhooks/${connectionId}`,
    webhook_url: `https://api.example.com/v1/billing/webhooks/${connectionId}`,
    configuration_version: webhookConfigured ? 2 : 1,
  };
}
