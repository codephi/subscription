import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ItemResponse,
  PriceVersionResponse,
  ProductResponse,
} from "@/api/catalog-api";
import {
  runProductCreation,
  type ProductCreationProgress,
  type ProductWorkflowClient,
} from "./catalog-product-workflow";
import { newCatalogProductDraft } from "./catalog-product";

const productId = "00000000-0000-4000-8000-000000000001";
const itemId = "00000000-0000-4000-8000-000000000002";
const priceId = "00000000-0000-4000-8000-000000000003";

class FakeProductWorkflowClient implements ProductWorkflowClient {
  operations: string[] = [];
  priceState: PriceVersionResponse["state"] = "DRAFT";
  itemStatus: ItemResponse["status"] = "INACTIVE";
  productStatus: ProductResponse["status"] = "INACTIVE";
  createProduct: ProductWorkflowClient["createProduct"] = async () => {
    this.operations.push("create-product");
    return product();
  };
  createItem: ProductWorkflowClient["createItem"] = async () => {
    this.operations.push("create-item");
    return item();
  };
  createUnitPrice: ProductWorkflowClient["createUnitPrice"] = async () => {
    this.operations.push("create-price");
    return price();
  };
  createTieredPrice: ProductWorkflowClient["createTieredPrice"] = async () =>
    price();
  publishPriceVersion: ProductWorkflowClient["publishPriceVersion"] =
    async () => {
      this.operations.push("publish-price");
      this.priceState = "ACTIVE";
      return price({ state: "ACTIVE" });
    };
  getProduct: ProductWorkflowClient["getProduct"] = async () => {
    this.operations.push("read-product");
    return product({ status: this.productStatus });
  };
  getItem: ProductWorkflowClient["getItem"] = async () => {
    this.operations.push("read-item");
    return item({ status: this.itemStatus });
  };
  getPrice: ProductWorkflowClient["getPrice"] = async () => {
    this.operations.push("read-price");
    return price({ state: this.priceState });
  };
  updateProduct: ProductWorkflowClient["updateProduct"] = async () => {
    this.operations.push("activate-product");
    this.productStatus = "ACTIVE";
    return product({ status: "ACTIVE" });
  };
  updateItem: ProductWorkflowClient["updateItem"] = async () => {
    this.operations.push("activate-item");
    this.itemStatus = "ACTIVE";
    return item({ status: "ACTIVE" });
  };
}

describe("runProductCreation", () => {
  let client: FakeProductWorkflowClient;
  let saved: ProductCreationProgress[];

  beforeEach(() => {
    client = new FakeProductWorkflowClient();
    saved = [];
  });

  it("creates and publishes the product graph in dependency order", async () => {
    const initial = progress();
    const id = await runProductCreation(initial, true, client, (state) =>
      saved.push(state),
    );

    expect(id).toBe(productId);
    expect(client.operations).toEqual([
      "create-product",
      "create-item",
      "create-price",
      "read-price",
      "publish-price",
      "read-item",
      "activate-item",
      "read-product",
      "activate-product",
    ]);
    expect(saved.at(-1)?.productId).toBe(productId);
    expect(saved.at(-1)?.step).toBeNull();
  });

  it("does not retry a creation whose response was lost", async () => {
    const initial = { ...progress(), step: "create:product", uncertain: true };

    await expect(
      runProductCreation(initial, false, client, vi.fn()),
    ).rejects.toThrow("Confira a lista");
    expect(client.operations).toEqual([]);
  });

  it("saves entitlement access products without creating metered items", async () => {
    const draft = newCatalogProductDraft();
    draft.name = "Feature access";
    draft.usageModel = "ENTITLEMENT_ONLY";
    const result = await runProductCreation(
      { ...progress(), draft },
      false,
      client,
      vi.fn(),
    );

    expect(result).toBe(productId);
    expect(client.operations).toEqual(["create-product"]);
  });

  it("checks a price after a lost publish response before continuing", async () => {
    client.priceState = "ACTIVE";
    client.itemStatus = "ACTIVE";
    client.productStatus = "ACTIVE";
    const initial = {
      ...progress(),
      productId,
      itemIds: { ["draft-item"]: itemId },
      priceIds: { ["draft-item"]: priceId },
      step: "publish:draft-item",
      uncertain: true,
    };

    await runProductCreation(initial, true, client, (state) =>
      saved.push(state),
    );

    expect(client.operations).toContain("read-price");
    expect(client.operations).not.toContain("publish-price");
  });
});

function progress(): ProductCreationProgress {
  const draft = newCatalogProductDraft();
  draft.name = "Product";
  draft.items[0]!.name = "Requests";
  draft.items[0]!.creditUnits = "2";
  return {
    draft,
    productId: null,
    itemIds: {},
    priceIds: {},
    step: null,
    uncertain: false,
    publishing: false,
  };
}

function product(overrides: Partial<ProductResponse> = {}): ProductResponse {
  return {
    product_id: productId,
    name: "Product",
    description: null,
    usage_model: "CREDIT_METERED",
    status: "INACTIVE",
    version: 1,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function item(overrides: Partial<ItemResponse> = {}): ItemResponse {
  return {
    item_id: itemId,
    product_id: productId,
    parent_item_id: null,
    name: "Requests",
    unit_name: "unidade",
    quantity_scale: "1",
    status: "INACTIVE",
    version: 1,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function price(
  overrides: Partial<PriceVersionResponse> = {},
): PriceVersionResponse {
  return {
    price_version_id: priceId,
    item_id: itemId,
    pricing_model: "unit",
    unit_block_size: "1",
    credit_units: "2",
    effective_from: "2026-01-01T00:00:00Z",
    effective_until: null,
    accumulation_cycle: null,
    tiers: [],
    state: "DRAFT",
    version: 1,
    created_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}
