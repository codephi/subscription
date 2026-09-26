import { api, unwrapResponse, ApiRequestError } from "./client";
import type { components } from "./generated";

export type ProductResponse = components["schemas"]["ProductResponse"];
export type ItemResponse = components["schemas"]["ItemResponse"];
export type PriceVersionResponse = components["schemas"]["PriceVersionResponse"];
export type CatalogEntryResponse = components["schemas"]["CatalogEntryResponse"];

/** Page catalog entries; e.g. `listCatalogEntries("products")`. */
export async function listCatalogEntries(
  kind: string,
  cursor?: string,
  parentId?: string,
) {
  return unwrapResponse(
    await api.GET("/v1/admin/catalog/{kind}", {
      params: {
        path: { kind },
        query: { cursor, parent_id: parentId, limit: 20 },
      },
    }),
  );
}

/** Read every page of one catalog kind; e.g. `listAllCatalogEntries("items", productId)`. */
export async function listAllCatalogEntries(
  kind: string,
  parentId?: string,
): Promise<CatalogEntryResponse[]> {
  const entries: CatalogEntryResponse[] = [];
  let cursor: string | undefined;
  do {
    const page = await listCatalogEntries(kind, cursor, parentId);
    entries.push(...page.items);
    cursor = page.next_cursor ?? undefined;
  } while (cursor);
  return entries;
}

/** Read a catalog entry through its public contract; e.g. `getCatalogDetail("items", id)`. */
export async function getCatalogDetail(
  kind: string,
  id: string,
): Promise<object> {
  switch (kind) {
    case "products":
      return unwrapResponse(
        await api.GET("/v1/products/{product_id}", {
          params: { path: { product_id: id } },
        }),
      );
    case "items":
      return unwrapResponse(
        await api.GET("/v1/items/{item_id}", {
          params: { path: { item_id: id } },
        }),
      );
    case "prices":
      return unwrapResponse(
        await api.GET("/v1/price-versions/{price_id}", {
          params: { path: { price_id: id } },
        }),
      );
    case "subscriptions":
      return unwrapResponse(
        await api.GET("/v1/subscriptions/{subscription_id}", {
          params: { path: { subscription_id: id } },
        }),
      );
    case "plans":
      return unwrapResponse(
        await api.GET("/v1/subscription-plans/{plan_id}", {
          params: { path: { plan_id: id } },
        }),
      );
    case "on-demand":
      return unwrapResponse(
        await api.GET("/v1/on-demand-plans/{on_demand_plan_id}", {
          params: { path: { on_demand_plan_id: id } },
        }),
      );
    case "policies":
      return unwrapResponse(
        await api.GET("/v1/admission-policies/{policy_version_id}", {
          params: { path: { policy_version_id: id } },
        }),
      );
    default:
      throw new ApiRequestError(
        422,
        "invalid_catalog_kind",
        `Tipo ${kind} desconhecido.`,
      );
  }
}

/** Create a product; e.g. `createProduct({ name, usage_model })`. */
export async function createProduct(body: {
  name: string;
  description?: string | null;
  usage_model: "CREDIT_METERED" | "ENTITLEMENT_ONLY";
}) {
  return unwrapResponse(await api.POST("/v1/products", { body }));
}

/** Read a product through its public contract; e.g. `getProduct(productId)`. */
export async function getProduct(productId: string): Promise<ProductResponse> {
  return unwrapResponse(
    await api.GET("/v1/products/{product_id}", {
      params: { path: { product_id: productId } },
    }),
  );
}

/** Update a product with optimistic concurrency; e.g. `updateProductStatus(product)`. */
export async function updateProduct(
  productId: string,
  body: components["schemas"]["UpdateProductRequest"],
) {
  return unwrapResponse(
    await api.PATCH("/v1/products/{product_id}", {
      params: { path: { product_id: productId } },
      body,
    }),
  );
}

/** Create an item under a product; e.g. `createItem(productId, body)`. */
export async function createItem(
  productId: string,
  body: {
    name: string;
    parent_item_id?: string | null;
    unit_name?: string | null;
    quantity_scale?: string | null;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/products/{product_id}/items", {
      params: { path: { product_id: productId } },
      body,
    }),
  );
}

/** Update an item with optimistic concurrency; e.g. `updateItemStatus(item)`. */
export async function updateItem(
  itemId: string,
  body: components["schemas"]["UpdateItemRequest"],
) {
  return unwrapResponse(
    await api.PATCH("/v1/items/{item_id}", {
      params: { path: { item_id: itemId } },
      body,
    }),
  );
}

/** Read one item through its public contract; e.g. `getItem(itemId)`. */
export async function getItem(itemId: string): Promise<ItemResponse> {
  return unwrapResponse(
    await api.GET("/v1/items/{item_id}", {
      params: { path: { item_id: itemId } },
    }),
  );
}

/** Read one price version through its public contract; e.g. `getPriceVersion(priceId)`. */
export async function getPriceVersion(
  priceId: string,
): Promise<PriceVersionResponse> {
  return unwrapResponse(
    await api.GET("/v1/price-versions/{price_id}", {
      params: { path: { price_id: priceId } },
    }),
  );
}

/** Create a unit price draft; e.g. `createUnitPrice(itemId, body)`. */
export async function createUnitPrice(
  itemId: string,
  body: {
    unit_block_size: string;
    credit_units: string;
    effective_from: string;
    effective_until?: string | null;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/items/{item_id}/price-versions", {
      params: { path: { item_id: itemId } },
      body: {
        pricing_model: "unit",
        ...body,
        accumulation_cycle: null,
        tiers: [],
      },
    }),
  );
}

/** Create a tiered price draft; e.g. `createTieredPrice(itemId, body)`. */
export async function createTieredPrice(
  itemId: string,
  body: {
    effective_from: string;
    effective_until?: string | null;
    accumulation_cycle?: { anchor_at: string; recurrence_rule: string } | null;
    tiers: {
      from_accumulated_units: string;
      to_accumulated_units: string | null;
      unit_block_size: string;
      credit_units: string;
    }[];
  },
) {
  return unwrapResponse(
    await api.POST("/v1/items/{item_id}/price-versions", {
      params: { path: { item_id: itemId } },
      body: {
        pricing_model: "tiered",
        unit_block_size: null,
        credit_units: null,
        ...body,
      },
    }),
  );
}

/** Publish a price draft; e.g. `publishPriceVersion(priceId)`. */
export async function publishPriceVersion(priceId: string) {
  return unwrapResponse(
    await api.POST("/v1/price-versions/{price_id}/publish", {
      params: { path: { price_id: priceId } },
    }),
  );
}

/** Create a subscription; e.g. `createSubscription({ name, subscription_model })`. */
export async function createSubscription(body: {
  name: string;
  subscription_model: "CREDIT_STRICT" | "CREDIT_FLEXIBLE" | "ENTITLEMENT_ONLY";
}) {
  return unwrapResponse(await api.POST("/v1/subscriptions", { body }));
}

/** Publish a subscription plan; e.g. `createSubscriptionPlan(id, body)`. */
export async function createSubscriptionPlan(
  subscriptionId: string,
  body: {
    name: string;
    commercial_model: "FREE" | "PAID";
    price_amount_minor: number | null;
    currency: string | null;
    recurrence: "NONE" | "WEEKLY" | "MONTHLY" | "QUARTERLY" | "ANNUALLY";
    admission_policy: "OPEN" | "APPROVAL_REQUIRED";
    admission_policy_version_id: string | null;
    accepted_payment_methods: string[];
    granted_credit_units: string;
    product_ids: string[];
  },
) {
  return unwrapResponse(
    await api.POST("/v1/subscriptions/{subscription_id}/plans", {
      params: { path: { subscription_id: subscriptionId } },
      body,
    }),
  );
}

/** Publish an admission policy; e.g. `createAdmissionPolicy(body)`. */
export async function createAdmissionPolicy(body: {
  policy_id: string;
  version: number;
  required_facts: ("EMAIL_VERIFIED" | "IDENTITY_VERIFIED")[];
}) {
  return unwrapResponse(await api.POST("/v1/admission-policies", { body }));
}

/** Publish an on-demand offer; e.g. `createOnDemandPlan(subscriptionId, body)`. */
export async function createOnDemandPlan(
  subscriptionId: string,
  body: {
    name: string;
    price_amount_minor: number;
    currency: string;
    credit_units: string;
  },
) {
  return unwrapResponse(
    await api.POST("/v1/subscriptions/{subscription_id}/on-demand-plans", {
      params: { path: { subscription_id: subscriptionId } },
      body,
    }),
  );
}

/** Revoke an immutable subscription offer; e.g. `revokeSubscriptionPlan(id, body)`. */
export async function revokeSubscriptionPlan(
  planId: string,
  body: { reason: string; actor_reference: string },
) {
  return unwrapResponse(
    await api.POST("/v1/subscription-plans/{plan_id}/revoke", {
      params: { path: { plan_id: planId } },
      body,
    }),
  );
}
