import createClient from "openapi-fetch";
import type { paths } from "./generated";

export const api = createClient<paths>({
  baseUrl: import.meta.env.VITE_API_BASE_URL ?? "",
});

export class ApiRequestError extends Error {
  readonly status: number;
  readonly code: string;
  readonly existingOperation?: {
    workspace_id: string;
    operation_kind: string;
    resource_id: string;
    transaction_id: string;
  };

  constructor(
    status: number,
    code: string,
    message: string,
    existingOperation?: {
      workspace_id: string;
      operation_kind: string;
      resource_id: string;
      transaction_id: string;
    },
  ) {
    super(message);
    this.name = "ApiRequestError";
    this.status = status;
    this.code = code;
    this.existingOperation = existingOperation;
  }
}

export function unwrapResponse<T>(response: {
  data?: T;
  error?: unknown;
  response: Response;
}): T {
  if (response.data !== undefined) return response.data;
  const body = response.error as
    | {
        error?: {
          code?: string;
          message?: string;
          existing_operation?: {
            workspace_id: string;
            operation_kind: string;
            resource_id: string;
            transaction_id: string;
          };
        };
      }
    | undefined;
  throw new ApiRequestError(
    response.response.status,
    body?.error?.code ?? "request_failed",
    body?.error?.message ??
      `A API respondeu com HTTP ${response.response.status}.`,
    body?.error?.existing_operation,
  );
}

/** Page workspace projections; e.g. `listWorkspaces()` for the search table. */
export async function listWorkspaces(cursor?: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/workspaces", {
      params: { query: { cursor, limit: 20 } },
    }),
  );
}

/** Read one workspace projection; e.g. `getWorkspace(id)` for its heading. */
export async function getWorkspace(workspaceId: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/workspaces/{workspace_id}", {
      params: { path: { workspace_id: workspaceId } },
    }),
  );
}

/** Create a workspace from the admin panel; e.g. `createWorkspace("ops@example.com")`. */
export async function createWorkspace(actorReference: string) {
  return unwrapResponse(
    await api.POST("/v1/admin/workspaces", {
      body: { actor_reference: actorReference },
    }),
  );
}

/** Page customer plans; e.g. `listCustomerPlans(id)` for the workspace panel. */
export async function listCustomerPlans(workspaceId: string, cursor?: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/workspaces/{workspace_id}/customer-plans", {
      params: {
        path: { workspace_id: workspaceId },
        query: { cursor, limit: 20 },
      },
    }),
  );
}

/** Read wallet hierarchy; e.g. `getWallets(id)` for available item wallets. */
export async function getWallets(workspaceId: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/wallets", {
      params: { path: { workspace_id: workspaceId } },
    }),
  );
}

/** Read wallet provisioning; e.g. `getProvisioning(id)` for readiness. */
export async function getProvisioning(workspaceId: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/wallet-provisioning", {
      params: { path: { workspace_id: workspaceId } },
    }),
  );
}

/** Page credit entries; e.g. `getStatement(id)` for recent credit activity. */
export async function getStatement(workspaceId: string, cursor?: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/customer-wallet/statement", {
      params: {
        path: { workspace_id: workspaceId },
        query: { cursor, limit: 20 },
      },
    }),
  );
}

/** Read an item meter on demand; e.g. `getItemMeter(id, itemId)` after selection. */
export async function getItemMeter(workspaceId: string, itemId: string) {
  return unwrapResponse(
    await api.GET("/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet", {
      params: { path: { workspace_id: workspaceId, item_id: itemId } },
    }),
  );
}

/** Page item usage entries; e.g. `getItemStatement(id, itemId)` in consumption. */
export async function getItemStatement(
  workspaceId: string,
  itemId: string,
  cursor?: string,
) {
  return unwrapResponse(
    await api.GET(
      "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement",
      {
        params: {
          path: { workspace_id: workspaceId, item_id: itemId },
          query: { cursor, limit: 20 },
        },
      },
    ),
  );
}

export {
  createBillingConnection,
  getBillingConfig,
  getBillingRecord,
  getOperations,
  grantDirectCredit,
  listBillingRecords,
  reconcileCredits,
  reconcileItemUsage,
  reconcileProvisioning,
  replayOutbox,
  updateBillingConfig,
} from "./billing-api";
export {
  createAdmissionPolicy,
  createItem,
  createOnDemandPlan,
  createProduct,
  createSubscription,
  createSubscriptionPlan,
  createTieredPrice,
  createUnitPrice,
  getCatalogDetail,
  listCatalogEntries,
  publishPriceVersion,
  revokeSubscriptionPlan,
} from "./catalog-api";
export {
  cancelCustomerPlan,
  getCustomerPlan,
  revokeCustomerPlan,
  transitionCustomerPlan,
} from "./plan-api";
export {
  listAuditEvents,
  listIntegrationInbox,
  replayInbox,
} from "./operations-api";
