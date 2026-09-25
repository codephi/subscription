import { api, unwrapResponse } from "./client";

/** Cancel a customer plan; e.g. `cancelCustomerPlan(workspaceId, planId)`. */
export async function cancelCustomerPlan(workspaceId: string, planId: string) {
  return unwrapResponse(
    await api.POST(
      "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/cancel",
      {
        params: {
          path: { workspace_id: workspaceId, customer_plan_id: planId },
        },
      },
    ),
  );
}

/** Read one customer plan before an action; e.g. `getCustomerPlan(id, planId)`. */
export async function getCustomerPlan(workspaceId: string, planId: string) {
  return unwrapResponse(
    await api.GET(
      "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}",
      {
        params: {
          path: { workspace_id: workspaceId, customer_plan_id: planId },
        },
      },
    ),
  );
}

/** Revoke a customer plan; e.g. `revokeCustomerPlan(workspaceId, planId, body)`. */
export async function revokeCustomerPlan(
  workspaceId: string,
  planId: string,
  body: { reason: string; actor_reference: string },
) {
  return unwrapResponse(
    await api.POST(
      "/v1/admin/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/revoke",
      {
        params: {
          path: { workspace_id: workspaceId, customer_plan_id: planId },
        },
        body,
      },
    ),
  );
}

/** Transition a customer plan using a stable key; e.g. `transitionCustomerPlan(id, planId, key, body)`. */
export async function transitionCustomerPlan(
  workspaceId: string,
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
      "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/plan-transitions",
      {
        params: {
          path: { workspace_id: workspaceId, customer_plan_id: planId },
          header: { "Idempotency-Key": key },
        },
        body,
      },
    ),
  );
}
