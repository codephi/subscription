import { expect, test } from "@playwright/test"

test("checkout keeps demo card fields in the browser and reports Subscription state", async ({ page }) => {
  await page.route("**/api/me", (route) => route.fulfill({ status: 401, json: { message: "sessão inválida" } }))
  await page.route("**/api/auth/login", (route) => route.fulfill({ status: 200, json: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" } }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: {
    account: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" },
    catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1 },
    wallet_statement: { items: [] }, eligibility: { access_allowed: false, balance_credit_units: "0" },
    meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
  } }))
  let checkoutBody: unknown
  await page.route("**/api/checkouts", async (route) => {
    checkoutBody = route.request().postDataJSON()
    expect(route.request().headers()["idempotency-key"]).toBeTruthy()
    await route.fulfill({ status: 202, json: { checkout_id: "checkout-1", status: "PENDING" } })
  })
  await page.route("**/api/checkouts/checkout-1", (route) => route.fulfill({ status: 200, json: { checkout_id: "checkout-1", status: "PAID" } }))

  await page.goto("/")
  await page.getByLabel("Usuário").fill("admin")
  await page.getByLabel("Senha").fill("admin")
  await page.getByRole("button", { name: "Entrar" }).click()
  await page.getByLabel("Cartão de demonstração").fill("4242 4242 4242 4242")
  await page.getByLabel("Validade fictícia").fill("12/30")
  await page.getByLabel("Código fictício").fill("123")
  await page.getByRole("button", { name: "Iniciar recarga" }).click()
  await expect.poll(() => checkoutBody).toEqual({ checkout_kind: "ON_DEMAND" })
  await expect(page.getByText(/Checkout paid/i)).toBeVisible()
})
