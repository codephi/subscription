import { api, unwrapResponse } from "./client";

/** Page audit evidence; e.g. `listAuditEvents(undefined, workspaceId)`. */
export async function listAuditEvents(
  cursor?: string,
  workspaceId?: string,
  action?: string,
) {
  return unwrapResponse(
    await api.GET("/v1/admin/audit-events", {
      params: {
        query: { cursor, workspace_id: workspaceId, action, limit: 20 },
      },
    }),
  );
}

/** Page Accounts integration inbox metadata; e.g. `listIntegrationInbox()`. */
export async function listIntegrationInbox(
  cursor?: string,
  workspaceId?: string,
  status?: string,
) {
  return unwrapResponse(
    await api.GET("/v1/admin/integration-inbox", {
      params: {
        query: { cursor, workspace_id: workspaceId, status, limit: 20 },
      },
    }),
  );
}

/** Replay an Accounts inbox event; e.g. `replayInbox(eventId)`. */
export async function replayInbox(eventId: string) {
  return unwrapResponse(
    await api.POST("/v1/admin/integration-inbox/{event_id}/replay", {
      params: { path: { event_id: eventId } },
    }),
  );
}
