import { expect, test } from "@playwright/test"

function account() {
  return { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" }
}

function dashboard(balance = "0") {
  return {
    account: account(),
    catalog: { plans: [{ plan_version_id: "plan-100", price_amount_minor: 2000, credit_units: 100 }, { plan_version_id: "plan-200", price_amount_minor: 4000, credit_units: 200 }, { plan_version_id: "plan-400", price_amount_minor: 6000, credit_units: 400 }], topup_price_per_credit_minor: 100, task_cost: 1 },
    wallet_statement: { items: [] },
    eligibility: { access_allowed: balance !== "0", balance_credit_units: balance },
    meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
  }
}

test("checkout asks Subscription for a hosted top-up and reports its confirmed state", async ({ page }) => {
  let checkoutBody: unknown
  let paid = false
  await page.route("**/api/me", (route) => route.fulfill({ status: 200, json: account() }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: dashboard(paid ? "25" : "0") }))
  await page.route("**/api/checkouts", async (route) => {
    checkoutBody = route.request().postDataJSON()
    expect(route.request().headers()["idempotency-key"]).toBeTruthy()
    await route.fulfill({ status: 202, json: { checkout_id: "checkout-1", status: "PENDING" } })
  })
  await page.route("**/api/checkouts/checkout-1", (route) => {
    paid = true
    return route.fulfill({ status: 200, json: { checkout_id: "checkout-1", status: "PAID" } })
  })

  await page.goto("/")
  await page.getByLabel("Créditos para adicionar").fill("25")
  await page.getByRole("button", { name: "Iniciar recarga" }).click()
  await expect.poll(() => checkoutBody).toEqual({ checkout_kind: "ON_DEMAND", topup_credits: 25 })
  await expect(page.getByText(/Checkout paid/i)).toBeVisible()
  await expect(page.locator(".balance-value")).toHaveText("25")
})

test("newly registered account already has its trial and never collects card details", async ({ page }) => {
  let registered = false
  await page.route("**/api/me", (route) => registered
    ? route.fulfill({ status: 200, json: account() })
    : route.fulfill({ status: 401, json: { message: "sessão inválida" } }))
  await page.route("**/api/auth/register", (route) => { registered = true; return route.fulfill({ status: 201, json: account() }) })
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: dashboard("10") }))

  await page.goto("/")
  await page.getByRole("tab", { name: "Criar conta" }).click()
  await page.locator("#register-username").fill("nova-conta")
  await page.locator("#register-password").fill("senha-local")
  await page.getByRole("button", { name: "Criar conta" }).click()
  await expect(page.getByText("10", { exact: true })).toBeVisible()
  await expect(page.getByText("Adicionar cartão")).toHaveCount(0)
  await expect(page.getByLabel("Créditos para adicionar")).toBeVisible()
})

test("pending checkout stops automatic polling and can be refreshed manually", async ({ page }) => {
  let statusChecks = 0
  await page.route("**/api/me", (route) => route.fulfill({ status: 200, json: account() }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: dashboard("10") }))
  await page.route("**/api/checkouts", (route) => route.fulfill({ status: 202, json: { checkout_id: "checkout-pending", status: "PENDING" } }))
  await page.route("**/api/checkouts/checkout-pending", (route) => {
    statusChecks += 1
    const status = statusChecks > 3 ? "PAID" : "PENDING"
    return route.fulfill({ status: 200, json: { checkout_id: "checkout-pending", status } })
  })

  await page.goto("/")
  await page.getByRole("button", { name: "Iniciar recarga" }).click()
  await expect(page.getByRole("button", { name: "Atualizar status" })).toBeVisible({ timeout: 10000 })
  expect(statusChecks).toBe(3)
  await page.getByRole("button", { name: "Atualizar status" }).click()
  await expect(page.getByText(/Checkout paid/i)).toBeVisible()
  expect(statusChecks).toBe(4)
})
