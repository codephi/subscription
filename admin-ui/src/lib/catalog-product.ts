export interface CatalogProductItemDraft {
  draftId: string;
  name: string;
  unitName: string;
  quantityScale: string;
  parentDraftId: string;
  pricingModel: "unit" | "tiered";
  unitBlockSize: string;
  creditUnits: string;
  effectiveFrom: string;
  effectiveUntil: string;
  accumulationAnchorAt: string;
  accumulationRecurrenceRule: string;
  tiers: PriceTierDraft[];
}

export interface PriceTierDraft {
  from: string;
  to: string;
  block: string;
  credits: string;
}

export interface CatalogProductDraft {
  name: string;
  description: string;
  usageModel: "CREDIT_METERED" | "ENTITLEMENT_ONLY";
  items: CatalogProductItemDraft[];
}

export function newCatalogItemDraft(name = ""): CatalogProductItemDraft {
  return {
    draftId: crypto.randomUUID(),
    name,
    unitName: "unidade",
    quantityScale: "1",
    parentDraftId: "",
    pricingModel: "unit",
    unitBlockSize: "1",
    creditUnits: "",
    effectiveFrom: "",
    effectiveUntil: "",
    accumulationAnchorAt: "",
    accumulationRecurrenceRule: "FREQ=MONTHLY;INTERVAL=1",
    tiers: [{ from: "0", to: "", block: "1", credits: "1" }],
  };
}

export function newCatalogProductDraft(): CatalogProductDraft {
  return {
    name: "",
    description: "",
    usageModel: "CREDIT_METERED",
    items: [newCatalogItemDraft()],
  };
}

export function validateCatalogProductDraft(
  draft: CatalogProductDraft,
  publish: boolean,
): string | null {
  if (!draft.name.trim()) return "Informe o nome do produto.";
  if (publish && draft.usageModel === "ENTITLEMENT_ONLY") {
    return "Produtos de acesso por assinatura ainda não podem ser publicados.";
  }
  if (publish && draft.items.length === 0) {
    return "Adicione ao menos um item com preço para publicar.";
  }
  if (draft.usageModel === "ENTITLEMENT_ONLY" && draft.items.length > 0) {
    return "Para acesso por assinatura, remova os itens de consumo antes de salvar.";
  }
  const hierarchyIssue = validateHierarchy(draft.items);
  if (hierarchyIssue) return hierarchyIssue;
  for (const [index, item] of draft.items.entries()) {
    const issue = validateItem(item, draft.items);
    if (issue) return `${item.name.trim() || `Item ${index + 1}`}: ${issue}`;
  }
  return null;
}

function validateItem(
  item: CatalogProductItemDraft,
  items: CatalogProductItemDraft[],
): string | null {
  if (!item.name.trim()) return "informe um nome.";
  if (item.parentDraftId && !items.some((candidate) => candidate.draftId === item.parentDraftId)) {
    return "selecione um item pai válido.";
  }
  if (item.parentDraftId === item.draftId) return "um item não pode ser seu próprio pai.";
  if (!positiveInteger(item.quantityScale)) return "a escala deve ser um inteiro positivo.";
  if (!positiveInteger(item.unitBlockSize) && item.pricingModel === "unit") {
    return "o tamanho do bloco deve ser um inteiro positivo.";
  }
  const dates = priceDates(item);
  if (dates.error) return dates.error;
  if (item.pricingModel === "unit") {
    if (!positiveInteger(item.creditUnits)) return "informe créditos inteiros positivos por bloco.";
    return null;
  }
  const tierIssue = validateTiers(item);
  if (tierIssue) return tierIssue;
  if (item.unitName.trim().length === 0) return "informe o nome da unidade.";
  return null;
}

function validateTiers(item: CatalogProductItemDraft): string | null {
  if (item.accumulationAnchorAt && !item.accumulationRecurrenceRule.trim()) {
    return "informe a recorrência do ciclo de acumulação.";
  }
  if (item.accumulationAnchorAt && !validCycleRule(item.accumulationRecurrenceRule)) {
    return "use uma recorrência FREQ e INTERVAL válida.";
  }
  const effectiveFrom = item.effectiveFrom ? new Date(item.effectiveFrom) : new Date();
  if (item.accumulationAnchorAt && new Date(item.accumulationAnchorAt) > effectiveFrom) {
    return "o ciclo de acumulação deve começar antes da vigência do preço.";
  }
  if (!item.tiers.length) return "adicione ao menos uma faixa.";
  let expectedStart = 0n;
  for (const [index, tier] of item.tiers.entries()) {
    if (!nonNegativeInteger(tier.from) || !positiveInteger(tier.block) || !positiveInteger(tier.credits)) {
      return `preencha os inteiros válidos da faixa ${index + 1}.`;
    }
    if (tier.to && !nonNegativeInteger(tier.to)) return `informe um limite final válido na faixa ${index + 1}.`;
    const start = BigInt(tier.from);
    const end = tier.to ? BigInt(tier.to) : null;
    if (start !== expectedStart || (end !== null && end <= start)) {
      return "as faixas devem ser contíguas, crescentes e começar em zero.";
    }
    if (end !== null && (end - start) % BigInt(tier.block) !== 0n) {
      return `a largura da faixa ${index + 1} deve ser divisível pelo bloco.`;
    }
    if ((index === item.tiers.length - 1) !== (end === null)) {
      return "somente a última faixa pode ter limite final aberto.";
    }
    expectedStart = end ?? expectedStart;
  }
  return null;
}

function validateHierarchy(items: CatalogProductItemDraft[]): string | null {
  const parents = new Map(items.map((item) => [item.draftId, item.parentDraftId]));
  for (const item of items) {
    const visited = new Set<string>();
    let current: string | undefined = item.draftId;
    while (current) {
      if (visited.has(current)) return "a hierarquia dos itens não pode conter ciclos.";
      visited.add(current);
      current = parents.get(current) || undefined;
    }
  }
  return null;
}

function validCycleRule(value: string): boolean {
  return /^FREQ=(DAILY|WEEKLY|MONTHLY|YEARLY);INTERVAL=[1-9]\d*$/.test(value.trim());
}

function priceDates(item: CatalogProductItemDraft): { error: string | null } {
  const start = item.effectiveFrom ? new Date(item.effectiveFrom) : new Date();
  if (Number.isNaN(start.valueOf())) return { error: "informe uma vigência inicial válida." };
  if (!item.effectiveUntil) return { error: null };
  const end = new Date(item.effectiveUntil);
  return Number.isNaN(end.valueOf()) || end <= start
    ? { error: "a vigência final deve ser posterior à inicial." }
    : { error: null };
}

function positiveInteger(value: string): boolean {
  return /^\d+$/.test(value) && BigInt(value) > 0n && BigInt(value) <= 9223372036854775807n;
}

function nonNegativeInteger(value: string): boolean {
  return /^\d+$/.test(value) && BigInt(value) <= 9223372036854775807n;
}
