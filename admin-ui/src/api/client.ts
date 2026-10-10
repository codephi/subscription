import createClient from "openapi-fetch";
import type { paths } from "./generated";

export const api = createClient<paths>({
  baseUrl: import.meta.env.VITE_API_BASE_URL ?? "",
});

export class ApiRequestError extends Error {
  readonly status: number;
  readonly code: string;
  readonly existingOperation?: {
    account_id: string;
    operation_kind: string;
    resource_id: string;
    transaction_id: string;
  };

  constructor(
    status: number,
    code: string,
    message: string,
    existingOperation?: {
      account_id: string;
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
            account_id: string;
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

/** Page account projections; e.g. `listAccounts()` for the search table. */
export async function listAccounts(cursor?: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/accounts", {
      params: { query: { cursor, limit: 20 } },
    }),
  );
}

/** Read one account projection; e.g. `getAccount(id)` for its heading. */
export async function getAccount(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/accounts/{account_id}", {
      params: { path: { account_id: accountId } },
    }),
  );
}

/** Create a account from the admin panel; e.g. `createAccount("ops@example.com")`. */
export async function createAccount(actorReference: string) {
  return unwrapResponse(
    await api.POST("/v1/admin/accounts", {
      body: { actor_reference: actorReference },
    }),
  );
}

/** Terminate a account while preserving its billing and audit history. */
export async function terminateAccount(accountId: string) {
  return unwrapResponse(
    await api.POST("/v1/admin/accounts/{account_id}/terminate", {
      params: { path: { account_id: accountId } },
    }),
  );
}

/** Page customer plans; e.g. `listCustomerPlans(id)` for the account panel. */
export async function listCustomerPlans(accountId: string, cursor?: string) {
  return unwrapResponse(
    await api.GET("/v1/admin/accounts/{account_id}/customer-plans", {
      params: {
        path: { account_id: accountId },
        query: { cursor, limit: 20 },
      },
    }),
  );
}

/** Read wallet hierarchy; e.g. `getWallets(id)` for available item wallets. */
export async function getWallets(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/wallets", {
      params: { path: { account_id: accountId } },
    }),
  );
}

/** Read wallet provisioning; e.g. `getProvisioning(id)` for readiness. */
export async function getProvisioning(accountId: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/wallet-provisioning", {
      params: { path: { account_id: accountId } },
    }),
  );
}

/** Page credit entries; e.g. `getStatement(id)` for recent credit activity. */
export async function getStatement(accountId: string, cursor?: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/customer-wallet/statement", {
      params: {
        path: { account_id: accountId },
        query: { cursor, limit: 20 },
      },
    }),
  );
}

/** Read an item meter on demand; e.g. `getItemMeter(id, itemId)` after selection. */
export async function getItemMeter(accountId: string, itemId: string) {
  return unwrapResponse(
    await api.GET("/v1/accounts/{account_id}/items/{item_id}/item-wallet", {
      params: { path: { account_id: accountId, item_id: itemId } },
    }),
  );
}

/** Page item usage entries; e.g. `getItemStatement(id, itemId)` in consumption. */
export async function getItemStatement(
  accountId: string,
  itemId: string,
  cursor?: string,
) {
  return unwrapResponse(
    await api.GET(
      "/v1/accounts/{account_id}/items/{item_id}/item-wallet/statement",
      {
        params: {
          path: { account_id: accountId, item_id: itemId },
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
