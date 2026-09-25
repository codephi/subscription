import { api, unwrapResponse } from "./client";

/** Read operational counters; e.g. `getOperations()` in the overview query. */
export async function getOperations() {
  return unwrapResponse(await api.GET("/v1/admin/billing/operations"));
}

/** Page evidence for one billing queue; e.g. `listBillingRecords("webhooks")`. */
export async function listBillingRecords(
  kind: string,
  cursor?: string,
  workspaceId?: string,
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
          workspace_id: workspaceId,
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
  workspaceId: string,
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
    await api.POST("/v1/workspaces/{workspace_id}/credits/direct", {
      params: {
        path: { workspace_id: workspaceId },
        header: { "Idempotency-Key": key },
      },
      body,
    }),
  );
}

/** Read workspace Billing switches; e.g. `getBillingConfig(id)`. */
export async function getBillingConfig(workspaceId: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/billing-config", {
      params: { path: { workspace_id: workspaceId } },
    }),
  );
}

/** Update workspace Billing switches with the observed version; e.g. `updateBillingConfig(id, body)`. */
export async function updateBillingConfig(
  workspaceId: string,
  body: {
    direct_credit_enabled: boolean;
    recurring_credit_enabled: boolean;
    expected_version: number;
  },
) {
  return unwrapResponse(
    await api.PUT("/v1/workspaces/{workspace_id}/billing-config", {
      params: { path: { workspace_id: workspaceId } },
      body,
    }),
  );
}

/** Register a Stripe connection using environment variable references; e.g. `createBillingConnection(id, body)`. */
export async function createBillingConnection(
  workspaceId: string,
  body: {
    provider: string;
    external_account_reference: string;
    secret_reference: string;
    webhook_secret_reference: string;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/workspaces/{workspace_id}/billing-connections", {
      params: { path: { workspace_id: workspaceId } },
      body,
    }),
  );
}

/** Compare wallet balance and ledger; e.g. `reconcileCredits(id)`. */
export async function reconcileCredits(workspaceId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/workspaces/{workspace_id}/customer-wallet/reconcile",
      { params: { path: { workspace_id: workspaceId } } },
    ),
  );
}

/** Reconcile wallet provisioning; e.g. `reconcileProvisioning(id)`. */
export async function reconcileProvisioning(workspaceId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile",
      { params: { path: { workspace_id: workspaceId } } },
    ),
  );
}

/** Compare item usage ledger and meter; e.g. `reconcileItemUsage(workspaceId, itemId)`. */
export async function reconcileItemUsage(workspaceId: string, itemId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/workspaces/{workspace_id}/items/{item_id}/usage/reconcile",
      { params: { path: { workspace_id: workspaceId, item_id: itemId } } },
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
