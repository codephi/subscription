import type { CatalogEntryResponse } from "@/api/catalog-api";
import { shortId } from "@/lib/format";

export interface SelectableEntry {
  id: string;
  label: string;
}

export function entriesWithIds(
  entries: CatalogEntryResponse[],
): SelectableEntry[] {
  return entries.map((entry) => ({
    id: entry.id,
    label: `${entry.name} · ${shortId(entry.id)}`,
  }));
}
