export const catalogKinds = {
  products: "Produtos",
  items: "Itens",
  prices: "Preços",
  subscriptions: "Assinaturas",
  plans: "Planos",
  "on-demand": "Ofertas avulsas",
  policies: "Políticas de admissão",
} as const;

export type CatalogKind = keyof typeof catalogKinds;

/** Validate a catalog route segment; e.g. `parseCatalogKind("products")`. */
export function parseCatalogKind(value?: string): CatalogKind | null {
  return value && value in catalogKinds ? (value as CatalogKind) : null;
}
