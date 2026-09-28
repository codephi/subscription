import { expect, test } from "@playwright/test";

const promotionId = "00000000-0000-4000-8000-000000000091";

test("operator creates a voucher with its default workspace limit", async ({ page }) => {
  let submitted: Record<string, unknown> | undefined;
  await page.route("**/v1/admin/vouchers", async (route) => {
    if (route.request().method() === "POST") {
      submitted = route.request().postDataJSON() as Record<string, unknown>;
      await route.fulfill({ status: 201, json: promotionResponse("VOUCHER") });
      return;
    }
    await route.fulfill({ json: { items: [], next_cursor: null } });
  });
  await page.route(`**/v1/admin/vouchers/${promotionId}`, async (route) =>
    route.fulfill({ json: promotionResponse("VOUCHER") }),
  );
  await page.route(`**/v1/admin/vouchers/${promotionId}/history`, async (route) =>
    route.fulfill({ json: { items: [] } }),
  );

  await page.goto("/promotions/new/vouchers");
  await page.getByRole("textbox", { name: "Código" }).fill(" boas-vindas ");
  await page.getByRole("textbox", { name: "Nome" }).fill("Boas vindas");
  await page.getByRole("textbox", { name: "Créditos concedidos por resgate" }).fill("250");
  await expect(page.getByLabel("Usos por workspace")).toHaveValue("1");
  await page.getByRole("button", { name: "Cadastrar promoção" }).click();

  await expect(page).toHaveURL(`/promotions/vouchers/${promotionId}`);
  await expect(page.getByText("250 créditos por uso")).toBeVisible();
  expect(submitted).toMatchObject({
    code: "boas-vindas",
    credit_units: "250",
    max_uses_per_workspace: 1,
    max_total_uses: null,
  });
});

test("operator creates a percentage coupon for both purchase types", async ({ page }) => {
  let submitted: Record<string, unknown> | undefined;
  await page.route("**/v1/admin/coupons", async (route) => {
    if (route.request().method() === "POST") {
      submitted = route.request().postDataJSON() as Record<string, unknown>;
      await route.fulfill({ status: 201, json: promotionResponse("COUPON") });
      return;
    }
    await route.fulfill({ json: { items: [], next_cursor: null } });
  });
  await page.route(`**/v1/admin/coupons/${promotionId}`, async (route) =>
    route.fulfill({ json: promotionResponse("COUPON") }),
  );
  await page.route(`**/v1/admin/coupons/${promotionId}/history`, async (route) =>
    route.fulfill({ json: { items: [] } }),
  );

  await page.goto("/promotions/new/coupons");
  await page.getByRole("textbox", { name: "Código" }).fill("LANCAMENTO");
  await page.getByRole("textbox", { name: "Nome" }).fill("Lançamento");
  await page.getByRole("spinbutton", { name: "Desconto em %" }).fill("25");
  await page.getByRole("checkbox", { name: "Compra de créditos" }).click();
  await page.getByRole("button", { name: "Cadastrar promoção" }).click();

  await expect(page).toHaveURL(`/promotions/coupons/${promotionId}`);
  await expect(page.getByText("25%")).toBeVisible();
  expect(submitted).toMatchObject({
    code: "LANCAMENTO",
    discount_kind: "PERCENTAGE",
    discount_value: 2500,
    currency: null,
    applies_to_initial: true,
    applies_to_on_demand: true,
  });
});

function promotionResponse(kind: "VOUCHER" | "COUPON") {
  return {
    promotion_id: promotionId,
    promotion_kind: kind,
    code: kind === "VOUCHER" ? "BOAS-VINDAS" : "LANCAMENTO",
    name: kind === "VOUCHER" ? "Boas vindas" : "Lançamento",
    description: null,
    status: "ACTIVE",
    version: 1,
    valid_from: null,
    valid_until: null,
    max_total_uses: null,
    max_uses_per_workspace: 1,
    completed_uses: 0,
    reserved_uses: 0,
    availability: "AVAILABLE",
    credit_units: kind === "VOUCHER" ? "250" : null,
    discount_kind: kind === "COUPON" ? "PERCENTAGE" : null,
    discount_value: kind === "COUPON" ? 2500 : null,
    currency: null,
    applies_to_initial: kind === "COUPON" ? true : null,
    applies_to_on_demand: kind === "COUPON" ? true : null,
    created_at: "2026-09-28T12:00:00Z",
    updated_at: "2026-09-28T12:00:00Z",
  };
}
