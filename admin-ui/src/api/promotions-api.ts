import { api, unwrapResponse } from "./client";
import type { components } from "./generated";

type PromotionList = {
  cursor?: string;
  status?: string;
  search?: string;
  limit?: number;
};
type PromotionUpdate = components["schemas"]["UpdatePromotionRequest"];

export async function listPromotions(
  kind: "vouchers" | "coupons",
  query: PromotionList = {},
) {
  if (kind === "vouchers") {
    return unwrapResponse(
      await api.GET("/v1/admin/vouchers", { params: { query } }),
    );
  }
  return unwrapResponse(
    await api.GET("/v1/admin/coupons", { params: { query } }),
  );
}

export async function getPromotion(kind: "vouchers" | "coupons", id: string) {
  if (kind === "vouchers") {
    return unwrapResponse(
      await api.GET("/v1/admin/vouchers/{id}", { params: { path: { id } } }),
    );
  }
  return unwrapResponse(
    await api.GET("/v1/admin/coupons/{id}", { params: { path: { id } } }),
  );
}

export async function getPromotionHistory(
  kind: "vouchers" | "coupons",
  id: string,
) {
  if (kind === "vouchers") {
    return unwrapResponse(
      await api.GET("/v1/admin/vouchers/{id}/history", {
        params: { path: { id } },
      }),
    );
  }
  return unwrapResponse(
    await api.GET("/v1/admin/coupons/{id}/history", {
      params: { path: { id } },
    }),
  );
}

export async function createVoucher(
  body: components["schemas"]["CreateVoucherRequest"],
) {
  return unwrapResponse(await api.POST("/v1/admin/vouchers", { body }));
}

export async function createCoupon(
  body: components["schemas"]["CreateCouponRequest"],
) {
  return unwrapResponse(await api.POST("/v1/admin/coupons", { body }));
}

export async function updatePromotion(
  kind: "vouchers" | "coupons",
  id: string,
  body: PromotionUpdate,
) {
  if (kind === "vouchers") {
    return unwrapResponse(
      await api.PATCH("/v1/admin/vouchers/{id}", {
        params: { path: { id } },
        body,
      }),
    );
  }
  return unwrapResponse(
    await api.PATCH("/v1/admin/coupons/{id}", {
      params: { path: { id } },
      body,
    }),
  );
}

export async function redeemVoucher(
  workspaceId: string,
  idempotencyKey: string,
  body: components["schemas"]["RedeemVoucherRequest"],
) {
  return unwrapResponse(
    await api.POST("/v1/workspaces/{workspace_id}/voucher-redemptions", {
      params: {
        path: { workspace_id: workspaceId },
        header: { "Idempotency-Key": idempotencyKey },
      },
      body,
    }),
  );
}
