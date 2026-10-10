import { api, unwrapResponse } from "./client";

/** Cancel a customer plan; e.g. `cancelCustomerPlan(accountId, planId)`. */
export async function cancelCustomerPlan(accountId: string, planId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/accounts/{account_id}/customer-plans/{customer_plan_id}/cancel",
      {
        params: {
          path: { account_id: accountId, customer_plan_id: planId },
        },
      },
    ),
  );
}

/** Read one customer plan before an action; e.g. `getCustomerPlan(id, planId)`. */
export async function getCustomerPlan(accountId: string, planId: string) {
  return unwrapResponse(
    await api.GET(
      "/v1/accounts/{account_id}/customer-plans/{customer_plan_id}",
      {
        params: {
          path: { account_id: accountId, customer_plan_id: planId },
        },
      },
    ),
  );
}

/** Revoke a customer plan; e.g. `revokeCustomerPlan(accountId, planId, body)`. */
export async function revokeCustomerPlan(
  accountId: string,
  planId: string,
  body: { reason: string; actor_reference: string },
) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/accounts/{account_id}/customer-plans/{customer_plan_id}/revoke",
      {
        params: {
          path: { account_id: accountId, customer_plan_id: planId },
        },
        body,
      },
    ),
  );
}

/** Transition a customer plan using a stable key; e.g. `transitionCustomerPlan(id, planId, key, body)`. */
export async function transitionCustomerPlan(
  accountId: string,
  planId: string,
  key: string,
  body: {
    new_plan_version_id: string;
    transition_kind: "UPGRADE" | "DOWNGRADE";
    payment_method_binding_id?: string | null;
    transaction_id: string;
    actor_reference: string;
  },
) {
  return unwrapResponse(
    await api.POST(
      "/v1/accounts/{account_id}/customer-plans/{customer_plan_id}/plan-transitions",
      {
        params: {
          path: { account_id: accountId, customer_plan_id: planId },
          header: { "Idempotency-Key": key },
        },
        body,
      },
    ),
  );
}
