import { api, unwrapResponse } from "./client";
import type { components } from "./generated";
import { listAccounts } from "./client";
import { listCatalogEntries } from "./catalog-api";

type QuoteRequest = components["schemas"]["CheckoutQuoteRequest"];
type CreateRequest = components["schemas"]["CreateCheckoutRequest"];

/** Quote a coupon against a published initial plan or on-demand credit offer. */
export async function quoteCheckout(accountId: string, body: QuoteRequest) {
  return unwrapResponse(
    await api.POST("/v1/accounts/{account_id}/checkout-quotes", {
      params: { path: { account_id: accountId } },
      body,
    }),
  );
}

/** Start an idempotent checkout and retain the server's checkout identifier. */
export async function createCheckout(
  accountId: string,
  idempotencyKey: string,
  body: CreateRequest,
) {
  return unwrapResponse(
    await api.POST("/v1/accounts/{account_id}/checkouts", {
      params: {
        path: { account_id: accountId },
        header: { "Idempotency-Key": idempotencyKey },
      },
      body,
    }),
  );
}

/** Recover a checkout after a lost response or page reload. */
export async function getCheckout(accountId: string, checkoutId: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/checkouts/{checkout_id}", {
      params: { path: { account_id: accountId, checkout_id: checkoutId } },
    }),
  );
}

export async function listCheckoutAccounts() {
  return listAccounts();
}

export async function listCheckoutPlans() {
  return listCatalogEntries("plans");
}

export async function listCheckoutOffers(subscriptionId?: string) {
  return listCatalogEntries("on-demand", undefined, subscriptionId);
}

export async function listCheckoutCustomerPlans(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/accounts/{account_id}/customer-plans", {
      params: { path: { account_id: accountId }, query: { limit: 100 } },
    }),
  );
}

export async function getCheckoutPaymentMethods(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/payment-method-bindings", {
      params: { path: { account_id: accountId } },
    }),
  );
}

export async function createCheckoutCustomerPlan(
  accountId: string,
  idempotencyKey: string,
  planVersionId: string,
  transactionId: string,
) {
  return unwrapResponse(
    await api.POST("/v1/accounts/{account_id}/customer-plans", {
      params: {
        path: { account_id: accountId },
        header: { "Idempotency-Key": idempotencyKey },
      },
      body: { plan_version_id: planVersionId, transaction_id: transactionId },
    }),
  );
}

export async function getCheckoutPlan(planId: string) {
  return unwrapResponse(
    await api.GET("/v1/subscription-plans/{plan_id}", {
      params: { path: { plan_id: planId } },
    }),
  );
}
