import { describe, expect, it } from "vitest";
import {
  newCatalogItemDraft,
  newCatalogProductDraft,
  catalogUnitOptions,
  knownCatalogUnit,
  validateCatalogProductDraft,
} from "./catalog-product";

describe("validateCatalogProductDraft", () => {
  it("offers common consumption units and retains custom units", () => {
    expect(catalogUnitOptions).toContain("requisição");
    expect(knownCatalogUnit("GB")).toBe(true);
    expect(knownCatalogUnit("imagem")).toBe(false);
    expect(newCatalogProductDraft().items[0]?.unitBlockSize).toBe("1");
  });
  it("accepts a simple credit metered product without floating point conversion", () => {
    const draft = newCatalogProductDraft();
    draft.name = "API";
    draft.items[0]!.name = "Requests";
    draft.items[0]!.creditUnits = "9007199254740993";

    expect(validateCatalogProductDraft(draft, true)).toBeNull();
  });

  it("allows saving a product without items but requires items for publication", () => {
    const draft = newCatalogProductDraft();
    draft.name = "API";
    draft.items = [];

    expect(validateCatalogProductDraft(draft, false)).toBeNull();
    expect(validateCatalogProductDraft(draft, true)).toContain(
      "ao menos um item",
    );
  });

  it("rejects invalid integers, incomplete prices, and invalid ranges", () => {
    const draft = newCatalogProductDraft();
    draft.name = "API";
    draft.items[0]!.name = "Requests";
    draft.items[0]!.creditUnits = "1.5";
    expect(validateCatalogProductDraft(draft, false)).toContain(
      "inteiros positivos",
    );

    draft.items[0]!.pricingModel = "tiered";
    draft.items[0]!.tiers = [
      { from: "0", to: "10", block: "3", credits: "1" },
      { from: "10", to: "", block: "1", credits: "1" },
    ];
    expect(validateCatalogProductDraft(draft, false)).toContain(
      "divisível pelo bloco",
    );
  });

  it("rejects circular parent references before creating any catalog record", () => {
    const draft = newCatalogProductDraft();
    draft.name = "API";
    const first = draft.items[0]!;
    const second = newCatalogItemDraft("Batches");
    draft.items.push(second);
    first!.name = "Requests";
    first!.creditUnits = "1";
    second!.name = "Batches";
    second!.creditUnits = "1";
    first!.parentDraftId = second!.draftId;
    second!.parentDraftId = first!.draftId;

    expect(validateCatalogProductDraft(draft, false)).toContain(
      "não pode conter ciclos",
    );
  });

  it("preserves hidden metered draft values for entitlement products and keeps them unpublished", () => {
    const draft = newCatalogProductDraft();
    draft.name = "Feature access";
    draft.usageModel = "ENTITLEMENT_ONLY";
    draft.items[0]!.creditUnits = "not-used-while-access-only";

    expect(validateCatalogProductDraft(draft, false)).toBeNull();
    expect(validateCatalogProductDraft(draft, true)).toContain(
      "não podem ser publicados",
    );
  });
});
