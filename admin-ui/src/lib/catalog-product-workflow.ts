import {
  createItem,
  createProduct,
  createTieredPrice,
  createUnitPrice,
  getItem,
  getProduct,
  getPriceVersion,
  publishPriceVersion,
  updateItem,
  updateProduct,
  type ItemResponse,
  type PriceVersionResponse,
  type ProductResponse,
} from "@/api/catalog-api";
import { ApiRequestError } from "@/api/client";
import type { CatalogProductDraft, CatalogProductItemDraft } from "./catalog-product";

export interface ProductCreationProgress {
  draft: CatalogProductDraft;
  productId: string | null;
  itemIds: Record<string, string>;
  priceIds: Record<string, string>;
  step: string | null;
  uncertain: boolean;
  publishing: boolean;
}

export interface ProductWorkflowClient {
  createProduct: typeof createProduct;
  createItem: typeof createItem;
  createUnitPrice: typeof createUnitPrice;
  createTieredPrice: typeof createTieredPrice;
  publishPriceVersion: typeof publishPriceVersion;
  getProduct: (id: string) => Promise<ProductResponse>;
  getItem: (id: string) => Promise<ItemResponse>;
  getPrice: (id: string) => Promise<PriceVersionResponse>;
  updateProduct: typeof updateProduct;
  updateItem: typeof updateItem;
}

export const productWorkflowClient: ProductWorkflowClient = {
  createProduct,
  createItem,
  createUnitPrice,
  createTieredPrice,
  publishPriceVersion,
  getProduct,
  getItem,
  getPrice: getPriceVersion,
  updateProduct,
  updateItem,
};

export async function runProductCreation(
  initial: ProductCreationProgress,
  publish: boolean,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<string> {
  let progress = await recoverInterruptedStep(initial, client, save);
  progress = { ...progress, publishing: publish, uncertain: false };
  if (!progress.productId) {
    progress = await runCreate(
      "create:product",
      progress,
      save,
      () => client.createProduct({
        name: progress.draft.name.trim(),
        description: progress.draft.description.trim() || null,
        usage_model: progress.draft.usageModel,
      }),
      (next, response) => ({ ...next, productId: response.product_id }),
    );
  }
  for (const item of orderItems(progress.draft.items)) {
    progress = await createItemIfNeeded(progress, item, client, save);
    progress = await createPriceIfNeeded(progress, item, client, save);
    if (publish) progress = await publishPriceIfNeeded(progress, item, client, save);
    if (publish) progress = await activateItem(progress, item, client, save);
  }
  if (publish) await activateProduct(progress, client, save);
  return progress.productId!;
}

export function canResumeProductCreation(progress: ProductCreationProgress): boolean {
  return !progress.uncertain || Boolean(progress.step && !progress.step.startsWith("create:"));
}

async function recoverInterruptedStep(
  progress: ProductCreationProgress,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<ProductCreationProgress> {
  if (!progress.step) return progress;
  if (progress.step.startsWith("create:")) {
    throw new Error("Esta criação pode ter sido concluída sem resposta. Confira a lista de produtos antes de iniciar outra tentativa.");
  }
  const [action, draftId] = progress.step.split(":");
  if (action === "publish" && draftId && progress.priceIds[draftId]) {
    await client.getPrice(progress.priceIds[draftId]);
  } else if (action === "activate-item" && draftId && progress.itemIds[draftId]) {
    await client.getItem(progress.itemIds[draftId]);
  } else if (action === "activate-product" && progress.productId) {
    await client.getProduct(progress.productId);
  } else if (!progress.step.startsWith("read:")) {
    throw new Error("Confira o catálogo antes de continuar esta etapa.");
  }
  progress = { ...progress, step: null, uncertain: false };
  save(progress);
  return progress;
}

async function createItemIfNeeded(
  progress: ProductCreationProgress,
  item: CatalogProductItemDraft,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<ProductCreationProgress> {
  if (progress.itemIds[item.draftId]) return progress;
  const parentItemId = item.parentDraftId
    ? progress.itemIds[item.parentDraftId]
    : null;
  return runCreate(`create:item:${item.draftId}`, progress, save, () => client.createItem(
    progress.productId!,
    {
      name: item.name.trim(),
      parent_item_id: parentItemId,
      unit_name: item.unitName.trim(),
      quantity_scale: item.quantityScale,
    },
  ), (next, response) => ({
    ...next,
    itemIds: { ...next.itemIds, [item.draftId]: response.item_id },
  }));
}

async function createPriceIfNeeded(
  progress: ProductCreationProgress,
  item: CatalogProductItemDraft,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<ProductCreationProgress> {
  if (progress.draft.usageModel !== "CREDIT_METERED" || progress.priceIds[item.draftId]) return progress;
  const effectiveFrom = toIso(item.effectiveFrom) ?? new Date().toISOString();
  const effectiveUntil = toIso(item.effectiveUntil);
  const create = () => item.pricingModel === "unit"
    ? client.createUnitPrice(progress.itemIds[item.draftId], {
        unit_block_size: item.unitBlockSize,
        credit_units: item.creditUnits,
        effective_from: effectiveFrom,
        effective_until: effectiveUntil,
      })
    : client.createTieredPrice(progress.itemIds[item.draftId], {
        effective_from: effectiveFrom,
        effective_until: effectiveUntil,
        accumulation_cycle: item.accumulationAnchorAt
          ? { anchor_at: toIso(item.accumulationAnchorAt)!, recurrence_rule: item.accumulationRecurrenceRule.trim() }
          : null,
        tiers: item.tiers.map((tier) => ({
          from_accumulated_units: tier.from,
          to_accumulated_units: tier.to || null,
          unit_block_size: tier.block,
          credit_units: tier.credits,
        })),
      });
  return runCreate(`create:price:${item.draftId}`, progress, save, create, (next, response) => ({
    ...next,
    priceIds: { ...next.priceIds, [item.draftId]: response.price_version_id },
  }));
}

async function publishPriceIfNeeded(
  progress: ProductCreationProgress,
  item: CatalogProductItemDraft,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<ProductCreationProgress> {
  const priceId = progress.priceIds[item.draftId];
  if (!priceId) return progress;
  const price = await recoverRead(`read:price:${item.draftId}`, progress, save, () => client.getPrice(priceId));
  if (price.state !== "DRAFT") return progress;
  return runMutation(`publish:${item.draftId}`, progress, save, () => client.publishPriceVersion(priceId));
}

async function activateItem(
  progress: ProductCreationProgress,
  item: CatalogProductItemDraft,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<ProductCreationProgress> {
  const current = await recoverRead(`read:item:${item.draftId}`, progress, save, () => client.getItem(progress.itemIds[item.draftId]));
  if (current.status === "ACTIVE") return progress;
  return runMutation(`activate-item:${item.draftId}`, progress, save, () => client.updateItem(current.item_id, {
    status: "ACTIVE",
    expected_version: current.version,
  }));
}

async function activateProduct(
  progress: ProductCreationProgress,
  client: ProductWorkflowClient,
  save: (progress: ProductCreationProgress) => void,
): Promise<void> {
  const current = await recoverRead("read:product", progress, save, () => client.getProduct(progress.productId!));
  if (current.status === "ACTIVE") return;
  await runMutation("activate-product", progress, save, () => client.updateProduct(current.product_id, {
    status: "ACTIVE",
    expected_version: current.version,
  }));
}

async function recoverRead<T>(
  step: string,
  progress: ProductCreationProgress,
  save: (progress: ProductCreationProgress) => void,
  read: () => Promise<T>,
): Promise<T> {
  progress = { ...progress, step, uncertain: true };
  save(progress);
  try {
    const result = await read();
    save({ ...progress, step: null, uncertain: false });
    return result;
  } catch (error) {
    if (error instanceof ApiRequestError) save({ ...progress, step: null, uncertain: false });
    throw error;
  }
}

async function runCreate<T>(
  step: string,
  progress: ProductCreationProgress,
  save: (progress: ProductCreationProgress) => void,
  create: () => Promise<T>,
  record: (progress: ProductCreationProgress, response: T) => ProductCreationProgress = (current) => current,
): Promise<ProductCreationProgress> {
  progress = { ...progress, step, uncertain: true };
  save(progress);
  try {
    const response = await create();
    progress = { ...record(progress, response), step: null };
    save(progress);
    return progress;
  } catch (error) {
    const uncertain = !(error instanceof ApiRequestError);
    save({ ...progress, step: uncertain ? step : null, uncertain });
    throw error;
  }
}

async function runMutation<T>(
  step: string,
  progress: ProductCreationProgress,
  save: (progress: ProductCreationProgress) => void,
  mutate: () => Promise<T>,
): Promise<ProductCreationProgress> {
  progress = { ...progress, step, uncertain: true };
  save(progress);
  try {
    await mutate();
    progress = { ...progress, step: null, uncertain: false };
    save(progress);
    return progress;
  } catch (error) {
    if (error instanceof ApiRequestError) {
      progress = { ...progress, step: null, uncertain: false };
      save(progress);
    }
    throw error;
  }
}

function orderItems(items: CatalogProductItemDraft[]): CatalogProductItemDraft[] {
  const ordered: CatalogProductItemDraft[] = [];
  const pending = new Map(items.map((item) => [item.draftId, item]));
  while (pending.size) {
    const ready = [...pending.values()].find((item) => !item.parentDraftId || !pending.has(item.parentDraftId));
    if (!ready) throw new Error("A hierarquia dos itens contém uma dependência circular.");
    ordered.push(ready);
    pending.delete(ready.draftId);
  }
  return ordered;
}

function toIso(value: string): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) throw new Error(`Data inválida: ${value}`);
  return date.toISOString();
}
