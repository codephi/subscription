/** Format a UTC API timestamp in the viewer's timezone; e.g. `formatDate(iso)`. */
export function formatDate(value?: string | null): string {
  if (!value) return "—";
  return new Intl.DateTimeFormat("pt-BR", {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(value));
}

/** Preserve exact decimal units; e.g. `formatUnits("1250")`. */
export function formatUnits(value?: string | null): string {
  if (value === undefined || value === null) return "—";
  return BigInt(value).toLocaleString("pt-BR");
}

/** Shorten an identifier for tables; e.g. `shortId(uuid)`. */
export function shortId(value: string): string {
  return `${value.slice(0, 8)}…${value.slice(-4)}`;
}
