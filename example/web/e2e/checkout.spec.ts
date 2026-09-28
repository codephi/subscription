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
      catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1, topup_offers: [{ credit_units: 10, price_amount_minor: 1000 }, { credit_units: 25, price_amount_minor: 2500 }, { credit_units: 50, price_amount_minor: 5000 }] },
      payment_methods: [{ payment_method_binding_id: "binding-2", status: "ACTIVE", created_at: "2026-01-01T00:00:00Z" }],
      wallet_statement: { items: [] }, eligibility: { access_allowed: false, balance_credit_units: "0" },
      meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
    }
  }
}

test("checkout sends the saved payment binding and reports Subscription state", async ({ page }) => {
  let checkoutPaid = false
  await page.route("**/api/me", (route) => route.fulfill({ status: 200, json: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" } }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: {
    account: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" },
    catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1, topup_offers: [{ credit_units: 10, price_amount_minor: 1000 }, { credit_units: 25, price_amount_minor: 2500 }, { credit_units: 50, price_amount_minor: 5000 }] },
    payment_methods: [{ payment_method_binding_id: "binding-1", status: "ACTIVE", created_at: "2026-01-01T00:00:00Z" }],
    wallet_statement: { items: [] }, eligibility: { access_allowed: checkoutPaid, balance_credit_units: checkoutPaid ? "25" : "0" },
    meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
  } }))
  let checkoutBody: unknown
  let releaseCheckout: (() => void) | undefined
  const checkoutGate = new Promise<void>((resolve) => { releaseCheckout = resolve })
  await page.route("**/api/checkouts", async (route) => {
    checkoutBody = route.request().postDataJSON()
    expect(route.request().headers()["idempotency-key"]).toBeTruthy()
    await checkoutGate
    await route.fulfill({ status: 202, json: { checkout_id: "checkout-1", status: "PENDING" } })
  })
  await page.route("**/api/checkouts/checkout-1", (route) => {
    checkoutPaid = true
    return route.fulfill({ status: 200, json: { checkout_id: "checkout-1", status: "PAID" } })
  })

  await page.goto("/")
  await expect(page.getByLabel("Cartão salvo")).toHaveValue("binding-1")
  await page.getByText("25 créditos", { exact: true }).click()
  await page.getByRole("button", { name: "Iniciar recarga" }).click()
  await expect(page.getByText("Preparando sua recarga")).toBeVisible()
  await expect.poll(() => checkoutBody).toEqual({ checkout_kind: "ON_DEMAND", topup_credits: 25, payment_method_binding_id: "binding-1" })
  releaseCheckout?.()
  await expect(page.getByText(/Checkout paid/i)).toBeVisible()
  await expect(page.locator(".balance-value")).toHaveText("25")
})

test("a card form is validated by Subscription and its saved binding funds a top-up", async ({ page }) => {
  let bindingSaved = false
  await page.route("**/api/me", (route) => route.fulfill({ status: 200, json: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" } }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: {
    account: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" },
    catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1, topup_offers: [] },
    payment_methods: bindingSaved ? [{ payment_method_binding_id: "binding-1", status: "ACTIVE", created_at: "2026-01-01T00:00:00Z" }] : [], wallet_statement: { items: [] }, eligibility: { access_allowed: false, balance_credit_units: "0" },
    meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
  } }))
  let setupBody: unknown
  await page.route("**/api/payment-method-setup", async (route) => {
    setupBody = route.request().postDataJSON()
    expect(route.request().headers()["idempotency-key"]).toBeTruthy()
    bindingSaved = true
    await route.fulfill({ status: 200, json: { payment_method_binding_id: "binding-1", saved: true } })
  })
  let checkoutBody: unknown
  await page.route("**/api/checkouts", async (route) => {
    checkoutBody = route.request().postDataJSON()
    await route.fulfill({ status: 202, json: { checkout_id: "checkout-1", status: "PENDING" } })
  })
  await page.goto("/")
  await page.getByRole("button", { name: "Adicionar cartão" }).click()
  await page.getByLabel("Nome impresso no cartão").fill("Teste TaskLab")
  await page.getByLabel("Número do cartão").fill("4242 4242 4242 4242")
  await page.getByLabel("Mês de validade").fill("12")
  await page.getByLabel("Ano de validade").fill("2035")
  await page.getByLabel("Código de segurança").fill("123")
  await page.getByRole("checkbox").check()
  await page.getByRole("button", { name: "Validar e salvar cartão" }).click()

  await expect.poll(() => setupBody).toEqual({ cardholder_name: "Teste TaskLab", card_number: "4242424242424242", exp_month: 12, exp_year: 2035, cvc: "123", save_for_future: true })
  await expect(page.getByLabel("Cartão salvo")).toHaveValue("binding-1")
  await page.getByRole("button", { name: "Iniciar recarga" }).click()
  await expect.poll(() => checkoutBody).toEqual({ checkout_kind: "ON_DEMAND", topup_credits: 10, payment_method_binding_id: "binding-1" })
})

test("card form reports a provider rejection without leaving TaskLab", async ({ page }) => {
  await page.route("**/api/me", (route) => route.fulfill({ status: 200, json: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" } }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: {
    account: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" },
    catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1, topup_offers: [] },
    payment_methods: [], wallet_statement: { items: [] }, eligibility: { access_allowed: false, balance_credit_units: "0" },
    meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
  } }))
  await page.route("**/api/payment-method-setup", (route) => route.fulfill({ status: 422, json: { message: "Cartão recusado pelo provedor." } }))

  await page.goto("/")
  await page.getByRole("button", { name: "Adicionar cartão" }).click()
  await page.getByLabel("Nome impresso no cartão").fill("Teste TaskLab")
  await page.getByLabel("Número do cartão").fill("4000000000000002")
  await page.getByLabel("Mês de validade").fill("12")
  await page.getByLabel("Ano de validade").fill("2035")
  await page.getByLabel("Código de segurança").fill("123")
  await page.getByRole("button", { name: "Validar cartão" }).click()
  await expect(page.getByText("Cartão recusado pelo provedor.")).toBeVisible()
  await expect(page).toHaveURL("/")
})

test("card form honors the choice not to save for later", async ({ page }) => {
  await page.route("**/api/me", (route) => route.fulfill({ status: 200, json: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" } }))
  await page.route("**/api/dashboard", (route) => route.fulfill({ status: 200, json: {
    account: { username: "admin", plan_model: "PREPAID", customer_plan_id: "plan-1", workspace_id: "workspace-1" },
    catalog: { prepaid_price_minor: 1000, prepaid_credits: 10, subscription_price_minor: 2990, subscription_credits: 50, task_cost: 1, topup_offers: [] },
    payment_methods: [], wallet_statement: { items: [] }, eligibility: { access_allowed: false, balance_credit_units: "0" },
    meter: { next_block_credit_units: "1" }, item_statement: { items: [] }, checkouts: [], executions: [],
  } }))
  let setupBody: unknown
  await page.route("**/api/payment-method-setup", async (route) => {
    setupBody = route.request().postDataJSON()
    await route.fulfill({ status: 200, json: { payment_method_binding_id: null, saved: false } })
  })

  await page.goto("/")
  await page.getByRole("button", { name: "Adicionar cartão" }).click()
  await page.getByLabel("Nome impresso no cartão").fill("Teste TaskLab")
  await page.getByLabel("Número do cartão").fill("4242424242424242")
  await page.getByLabel("Mês de validade").fill("12")
  await page.getByLabel("Ano de validade").fill("2035")
  await page.getByLabel("Código de segurança").fill("123")
  await page.getByRole("button", { name: "Validar cartão" }).click()

  await expect.poll(() => setupBody).toEqual({ cardholder_name: "Teste TaskLab", card_number: "4242424242424242", exp_month: 12, exp_year: 2035, cvc: "123", save_for_future: false })
  await expect(page.getByText("Cartão validado, mas não salvo. Marque a opção para reutilizá-lo em recargas.")).toBeVisible()
})

test("onboarding sends a subscription intent with the saved payment binding", async ({ page }) => {
  const fakeApi = new FakeTaskLabOnboardingApi()
  await fakeApi.install(page)
  await page.goto("/")
  await page.getByRole("tab", { name: "Criar conta" }).click()
  await page.locator("#register-username").fill("nova-conta")
  await page.locator("#register-password").fill("senha-local")
  await page.getByRole("button", { name: "Criar conta" }).click()
  await page.getByRole("radio", { name: "Assinatura" }).click()
  await expect.poll(() => page.getByRole("button", { name: "Iniciar assinatura" }).isEnabled()).toBe(true)
  await page.getByRole("button", { name: "Iniciar assinatura" }).click()
  await expect.poll(() => fakeApi.checkoutIntent).toEqual({ checkout_kind: "INITIAL", payment_method_binding_id: "binding-2" })
  await expect(page.getByText(/Checkout paid/i)).toBeVisible()
})
