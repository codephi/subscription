import { api, unwrapResponse } from "./client";
import type { components } from "./generated";
import { listWorkspaces } from "./client";
import { listCatalogEntries } from "./catalog-api";

type QuoteRequest = components["schemas"]["CheckoutQuoteRequest"];
type CreateRequest = components["schemas"]["CreateCheckoutRequest"];

/** Quote a coupon against a published initial plan or on-demand credit offer. */
export async function quoteCheckout(workspaceId: string, body: QuoteRequest) {
  return unwrapResponse(
    await api.POST("/v1/workspaces/{workspace_id}/checkout-quotes", {
      params: { path: { workspace_id: workspaceId } },
      body,
    }),
  );
}

/** Start an idempotent checkout and retain the server's checkout identifier. */
export async function createCheckout(
  workspaceId: string,
  idempotencyKey: string,
  body: CreateRequest,
) {
  return unwrapResponse(
    await api.POST("/v1/workspaces/{workspace_id}/checkouts", {
      params: {
        path: { workspace_id: workspaceId },
        header: { "Idempotency-Key": idempotencyKey },
      },
      body,
    }),
  );
}

/** Recover a checkout after a lost response or page reload. */
export async function getCheckout(workspaceId: string, checkoutId: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/checkouts/{checkout_id}", {
      params: { path: { workspace_id: workspaceId, checkout_id: checkoutId } },
    }),
  );
}

export async function listCheckoutWorkspaces() {
  return listWorkspaces();
}

export async function listCheckoutPlans() {
  return listCatalogEntries("plans");
}

export async function listCheckoutOffers(subscriptionId?: string) {
  return listCatalogEntries("on-demand", undefined, subscriptionId);
}

export async function listCheckoutCustomerPlans(workspaceId: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/workspaces/{workspace_id}/customer-plans", {
      params: { path: { workspace_id: workspaceId }, query: { limit: 100 } },
    }),
  );
}

export async function getCheckoutPaymentMethods(workspaceId: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/payment-method-bindings", {
      params: { path: { workspace_id: workspaceId } },
    }),
  );
}

export async function createCheckoutCustomerPlan(
  workspaceId: string,
  idempotencyKey: string,
  planVersionId: string,
  transactionId: string,
) {
  return unwrapResponse(
    await api.POST("/v1/workspaces/{workspace_id}/customer-plans", {
      params: {
        path: { workspace_id: workspaceId },
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
