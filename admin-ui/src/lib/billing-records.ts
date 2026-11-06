export const billingKinds = {
  collections: "Cobranças",
  attempts: "Tentativas",
  payments: "Pagamentos",
  webhooks: "Webhooks",
  outbox: "Outbox",
  unmatched: "Não conciliados",
} as const;

export type BillingKind = keyof typeof billingKinds;

/** Validate a billing path segment; e.g. `parseBillingKind("webhooks")`. */
export function parseBillingKind(value?: string): BillingKind | null {
  return value && value in billingKinds ? (value as BillingKind) : null;
}

/** Return available status filters for a billing queue; e.g. `billingStatuses("outbox")`. */
export function billingStatuses(kind: BillingKind): string[] {
  const statuses: Record<BillingKind, string[]> = {
    collections: [
      "ACTIVE",
      "SCHEDULED",
      "COLLECTING",
      "PENDING_PAYMENT",
      "PAID",
      "EXHAUSTED",
      "EXPIRED",
      "CANCELED",
      "UNMATCHED",
    ],
    attempts: [
      "SCHEDULED",
      "STARTED",
      "PENDING",
      "REQUIRES_ACTION",
      "SUCCEEDED",
      "FAILED",
      "UNCERTAIN",
    ],
    payments: ["PENDING", "REQUIRES_ACTION", "CONFIRMED", "FAILED", "CANCELED"],
    webhooks: [
      "FAILURE",
      "UNPROCESSED",
      "APPLIED",
      "UNMATCHED",
      "REJECTED",
      "DUPLICATE",
    ],
    outbox: ["PENDING", "DELIVERED", "DEAD_LETTER"],
    unmatched: ["OPEN", "RECONCILED", "CLOSED_WITH_JUSTIFICATION"],
  };
  return statuses[kind];
}
