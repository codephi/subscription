import { expect, test, type Page } from "@playwright/test"

class FakeTaskLabSessionApi {
  async install(page: Page) {
    await page.route("**/api/me", (route) => route.fulfill({ status: 401, json: { message: "sessão inválida" } }))
  }
}

test("theme toggle switches modes and preserves the choice after reload", async ({ page }) => {
  await new FakeTaskLabSessionApi().install(page)
  await page.goto("/")
  await page.getByRole("button", { name: "Ativar tema escuro" }).click()
  await expect(page.locator("html")).toHaveClass(/dark/)
  await page.reload()
  await expect(page.locator("html")).toHaveClass(/dark/)
  await page.getByRole("button", { name: "Ativar tema claro" }).click()
  await expect(page.locator("html")).not.toHaveClass(/dark/)
})
