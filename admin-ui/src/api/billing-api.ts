import { api, unwrapResponse } from "./client";

/** Read the encrypted default Stripe configuration summary; secrets are never returned. */
export async function getDefaultStripeCredentials() {
  return unwrapResponse(await api.GET("/v1/admin/billing/stripe-defaults"));
}

/** Save default Stripe credentials used when provisioning new accounts. */
export async function updateDefaultStripeCredentials(body: {
  expected_version: number;
  secret_key?: string;
  webhook_secret?: string;
}) {
  return unwrapResponse(
    await api.PUT("/v1/admin/billing/stripe-defaults", { body }),
  );
}

/** List the available billing providers; e.g. `listIntegrationProviders()`. */
export async function listIntegrationProviders() {
  return unwrapResponse(await api.GET("/v1/admin/integrations/providers"));
}

/** List account integration summaries without secret values. */
export async function listAccountIntegrations(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/accounts/{account_id}/integrations", {
      params: { path: { account_id: accountId } },
    }),
  );
}

/** Configure a Stripe account; secrets are sent directly to the API. */
export async function createStripeIntegration(
  accountId: string,
  body: {
    secret_key: string;
    environment: string;
    existing_customer_reference?: string | null;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/admin/accounts/{account_id}/integrations", {
      params: { path: { account_id: accountId } },
      body,
    }),
  );
}

/** Save webhook configuration or rotate a Stripe API key. */
export async function updateStripeIntegration(
  accountId: string,
  connectionId: string,
  body: {
    expected_version: number;
    secret_key?: string;
    webhook_secret?: string;
  },
) {
  return unwrapResponse(
    await api.PATCH(
      "/v1/admin/accounts/{account_id}/integrations/{connection_id}",
      { params: { path: { account_id: accountId, connection_id: connectionId } }, body },
    ),
  );
}

/** Test saved Stripe credentials; e.g. `testStripeIntegration(accountId, id)`. */
export async function testStripeIntegration(accountId: string, connectionId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/accounts/{account_id}/integrations/{connection_id}/test",
      { params: { path: { account_id: accountId, connection_id: connectionId } } },
    ),
  );
}

/** Read operational counters; e.g. `getOperations()` in the overview query. */
export async function getOperations() {
  return unwrapResponse(await api.GET("/v1/admin/billing/operations"));
}

/** Page evidence for one billing queue; e.g. `listBillingRecords("webhooks")`. */
export async function listBillingRecords(
  kind: string,
  cursor?: string,
  accountId?: string,
  status?: string,
  collectionRequestId?: string,
  correlationId?: string,
) {
  return unwrapResponse(
    await api.GET("/v1/admin/billing/records/{kind}", {
      params: {
        path: { kind },
        query: {
          cursor,
          account_id: accountId,
          status,
          collection_request_id: collectionRequestId,
          correlation_id: correlationId,
          limit: 20,
        },
      },
    }),
  );
}

/** Read one billing record; e.g. `getBillingRecord("payments", id)`. */
export async function getBillingRecord(kind: string, id: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/billing/records/{kind}/{id}", {
      params: { path: { kind, id } },
    }),
  );
}

/** Grant direct credit with a stable key; e.g. `grantDirectCredit(id, key, body)`. */
export async function grantDirectCredit(
  accountId: string,
  key: string,
  body: {
    transaction_id: string;
    credit_units: string;
    external_reference?: string | null;
    description?: string | null;
    metadata: Record<string, never>;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/accounts/{account_id}/credits/direct", {
      params: {
        path: { account_id: accountId },
        header: { "Idempotency-Key": key },
      },
      body,
    }),
  );
}

/** Read account Billing switches; e.g. `getBillingConfig(id)`. */
export async function getBillingConfig(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/billing-config", {
      params: { path: { account_id: accountId } },
    }),
  );
}

/** Update account Billing switches with the observed version; e.g. `updateBillingConfig(id, body)`. */
export async function updateBillingConfig(
  accountId: string,
  body: {
    direct_credit_enabled: boolean;
    recurring_credit_enabled: boolean;
    expected_version: number;
  },
) {
  return unwrapResponse(
    await api.PUT("/v1/accounts/{account_id}/billing-config", {
      params: { path: { account_id: accountId } },
      body,
    }),
  );
}

/** Register a legacy environment-referenced Stripe connection. */
export async function createBillingConnection(
  accountId: string,
  body: {
    provider: string;
    external_account_reference: string;
    secret_reference: string;
    webhook_secret_reference: string;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/accounts/{account_id}/billing-connections", {
      params: { path: { account_id: accountId } },
      body,
    }),
  );
}

/** Compare wallet balance and ledger; e.g. `reconcileCredits(id)`. */
export async function reconcileCredits(accountId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/accounts/{account_id}/customer-wallet/reconcile",
      { params: { path: { account_id: accountId } } },
    ),
  );
}

/** Reconcile wallet provisioning; e.g. `reconcileProvisioning(id)`. */
export async function reconcileProvisioning(accountId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/accounts/{account_id}/wallet-provisioning/reconcile",
      { params: { path: { account_id: accountId } } },
    ),
  );
}

/** Compare item usage ledger and meter; e.g. `reconcileItemUsage(accountId, itemId)`. */
export async function reconcileItemUsage(accountId: string, itemId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/accounts/{account_id}/items/{item_id}/usage/reconcile",
      { params: { path: { account_id: accountId, item_id: itemId } } },
    ),
  );
}

/** Replay a dead letter after investigation; e.g. `replayOutbox(id)`. */
export async function replayOutbox(id: string) {
  const result = await api.POST("/v1/admin/outbox-events/{event_id}/replay", {
    params: { path: { event_id: id } },
  });
  if (result.response.status === 204) return true;
  return unwrapResponse(result);
}
