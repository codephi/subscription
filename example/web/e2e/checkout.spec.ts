import { expect, test, type Page, type Route } from "@playwright/test"

class FakeTaskLabOnboardingApi {
  checkoutIntent: unknown
  private registered = false
  private checkoutStarted = false

  async install(page: Page) {
    await page.route("**/api/me", (route) => {
      return this.registered
        ? route.fulfill({ status: 200, json: this.checkoutStarted ? this.subscriptionAccount() : this.newAccount() })
        : route.fulfill({ status: 401, json: { message: "sessão inválida" } })
    })
    await page.route("**/api/auth/register", (route) => {
      this.registered = true
      return route.fulfill({ status: 201, json: this.newAccount() })
    })
    await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: this.dashboard() }))
    await page.route("**/api/checkouts", (route) => this.createCheckout(route))
    await page.route("**/api/checkouts/checkout-1", (route) => route.fulfill({ status: 200, json: { checkout_id: "checkout-1", status: "PAID" } }))
  }

  private async createCheckout(route: Route) {
    this.checkoutStarted = true
    this.checkoutIntent = route.request().postDataJSON()
    await route.fulfill({ status: 202, json: { checkout_id: "checkout-1", status: "PENDING" } })
  }

  private newAccount() {
    return { username: "nova-conta", plan_model: null, customer_plan_id: null, workspace_id: "workspace-2" }
  }

  private subscriptionAccount() {
    return { ...this.newAccount(), plan_model: "SUBSCRIPTION", customer_plan_id: "customer-plan-2" }
  }

  private dashboard() {
    return {
      account: this.subscriptionAccount(),
      catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1 },
      wallet_statement: { items: [] }, eligibility: { access_allowed: false, balance_credit_units: "0" },
      meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
    }
  }
}

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

test("onboarding sends a subscription intent without fictitious card fields", async ({ page }) => {
  const fakeApi = new FakeTaskLabOnboardingApi()
  await fakeApi.install(page)
  await page.goto("/")
  await page.getByRole("tab", { name: "Criar conta" }).click()
  await page.locator("#register-username").fill("nova-conta")
  await page.locator("#register-password").fill("senha-local")
  await page.getByRole("button", { name: "Criar conta" }).click()
  await page.getByRole("radio", { name: "Assinatura" }).click()
  await page.getByLabel("Número do cartão").fill("4242 4242 4242 4242")
  await page.getByLabel("Validade fictícia").fill("12/30")
  await page.getByLabel("Código fictício").fill("123")
  await page.getByRole("button", { name: "Iniciar assinatura" }).click()
  await expect.poll(() => fakeApi.checkoutIntent).toEqual({ checkout_kind: "INITIAL" })
  await expect(page.getByText(/Checkout paid/i)).toBeVisible()
})
