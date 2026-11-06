import { useState } from "react";
import type {
  ItemResponse,
  PriceVersionResponse,
  ProductResponse,
} from "@/api/catalog-api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { formatDate, formatUnits } from "@/lib/format";
import { Pencil } from "lucide-react";
import { Link } from "react-router-dom";

export interface CatalogItemEditRequest {
  name: string;
  parent_item_id: string | null;
  unit_name: string | null;
  quantity_scale: string | null;
  status: ItemResponse["status"];
  expected_version: number;
}

interface CatalogItemCardProps {
  item: ItemResponse;
  items: ItemResponse[];
  prices: PriceVersionResponse[];
  usageModel: ProductResponse["usage_model"];
  saving: boolean;
  publishingPriceId: string | null;
  onSave: (request: CatalogItemEditRequest) => Promise<unknown>;
  onPublishPrice: (price: PriceVersionResponse) => void;
}

interface ItemDraft {
  name: string;
  parentItemId: string;
  unitName: string;
  quantityScale: string;
  status: ItemResponse["status"];
}

export function CatalogItemCard({
  item,
  items,
  prices,
  usageModel,
  saving,
  publishingPriceId,
  onSave,
  onPublishPrice,
}: CatalogItemCardProps) {
  const [draft, setDraft] = useState<ItemDraft | null>(null);
  const itemPrices = prices.filter((price) => price.item_id === item.item_id);
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex flex-wrap items-center gap-3">
          {item.name}
          <Badge variant="outline">{item.status}</Badge>
        </CardTitle>
        <CardDescription>
          {item.unit_name
            ? `${item.unit_name} · escala ${item.quantity_scale}`
            : "Escopo de acesso"}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {draft ? (
          <ItemEditor
            draft={draft}
            item={item}
            items={items}
            usageModel={usageModel}
            saving={saving}
            onChange={setDraft}
            onCancel={() => setDraft(null)}
            onSave={async () => {
              try {
                await onSave({
                  name: draft.name,
                  parent_item_id: draft.parentItemId || null,
                  unit_name: draft.unitName.trim() || null,
                  quantity_scale: draft.quantityScale || null,
                  status: draft.status,
                  expected_version: item.version,
                });
                setDraft(null);
              } catch {
                // The parent page displays the mutation error and keeps this draft open.
              }
            }}
          />
        ) : (
          <Button
            variant="outline"
            className="self-start"
            onClick={() => setDraft(itemToDraft(item))}
          >
            <Pencil data-icon="inline-start" /> Editar item
          </Button>
        )}
        {itemPrices.map((price) => (
          <div
            key={price.price_version_id}
            className="flex flex-wrap items-center justify-between gap-3 rounded-md border p-3"
          >
            <div className="text-sm">
              <p>Preço: {describePrice(price)}</p>
              <p className="text-muted-foreground">
                Vigência: {formatDate(price.effective_from)} até{" "}
                {formatDate(price.effective_until)}
              </p>
              <p className="text-muted-foreground">
                O bloco do preço é diferente da escala do item. Preços
                publicados são imutáveis.
              </p>
            </div>
            <Badge variant={price.state === "DRAFT" ? "outline" : "secondary"}>
              {price.state}
            </Badge>
            {price.state === "DRAFT" && (
              <Button
                size="sm"
                disabled={Boolean(publishingPriceId)}
                onClick={() => onPublishPrice(price)}
              >
                {publishingPriceId === price.price_version_id
                  ? "Publicando…"
                  : "Publicar preço"}
              </Button>
            )}
          </div>
        ))}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm text-muted-foreground">
            {itemPrices.length === 0
              ? "Este item ainda não tem preço."
              : "Para mudar a cobrança, crie uma nova versão de preço."}
          </p>
          <Button
            size="sm"
            variant="outline"
            nativeButton={false}
            render={<Link to={`/catalog/prices/new?item_id=${item.item_id}`} />}
          >
            Criar nova versão de preço
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function ItemEditor({
  draft,
  item,
  items,
  usageModel,
  saving,
  onChange,
  onCancel,
  onSave,
}: {
  draft: ItemDraft;
  item: ItemResponse;
  items: ItemResponse[];
  usageModel: ProductResponse["usage_model"];
  saving: boolean;
  onChange: (draft: ItemDraft) => void;
  onCancel: () => void;
  onSave: () => Promise<unknown>;
}) {
  const unitsValid =
    usageModel === "ENTITLEMENT_ONLY" ||
    (Boolean(draft.unitName.trim()) &&
      /^\d+$/.test(draft.quantityScale) &&
      BigInt(draft.quantityScale) > 0n);
  return (
    <div className="grid gap-4 rounded-md border p-4 md:grid-cols-2">
      <label
        className="flex flex-col gap-2 text-sm font-medium"
        htmlFor={`item-name-${item.item_id}`}
      >
        Nome
        <Input
          id={`item-name-${item.item_id}`}
          value={draft.name}
          onChange={(event) => onChange({ ...draft, name: event.target.value })}
        />
      </label>
      <label
        className="flex flex-col gap-2 text-sm font-medium"
        htmlFor={`item-parent-${item.item_id}`}
      >
        Item pai
        <select
          id={`item-parent-${item.item_id}`}
          className="h-9 rounded-md border bg-transparent px-3 text-sm"
          value={draft.parentItemId}
          onChange={(event) =>
            onChange({ ...draft, parentItemId: event.target.value })
          }
        >
          <option value="">Nenhum</option>
          {items
            .filter((candidate) => candidate.item_id !== item.item_id)
            .map((candidate) => (
              <option key={candidate.item_id} value={candidate.item_id}>
                {candidate.name}
              </option>
            ))}
        </select>
      </label>
      <label
        className="flex flex-col gap-2 text-sm font-medium"
        htmlFor={`item-unit-${item.item_id}`}
      >
        Unidade
        <Input
          id={`item-unit-${item.item_id}`}
          value={draft.unitName}
          disabled={usageModel === "ENTITLEMENT_ONLY"}
          onChange={(event) =>
            onChange({ ...draft, unitName: event.target.value })
          }
        />
      </label>
      <label
        className="flex flex-col gap-2 text-sm font-medium"
        htmlFor={`item-scale-${item.item_id}`}
      >
        Escala de quantidade
        <Input
          id={`item-scale-${item.item_id}`}
          inputMode="numeric"
          value={draft.quantityScale}
          disabled={usageModel === "ENTITLEMENT_ONLY"}
          onChange={(event) =>
            onChange({ ...draft, quantityScale: event.target.value })
          }
        />
      </label>
      <label
        className="flex flex-col gap-2 text-sm font-medium"
        htmlFor={`item-status-${item.item_id}`}
      >
        Status
        <select
          id={`item-status-${item.item_id}`}
          className="h-9 rounded-md border bg-transparent px-3 text-sm"
          value={draft.status}
          onChange={(event) =>
            onChange({
              ...draft,
              status: event.target.value as ItemResponse["status"],
            })
          }
        >
          <option value="INACTIVE">Inativo</option>
          <option value="ACTIVE">Ativo</option>
          <option value="ARCHIVED">Arquivado</option>
        </select>
      </label>
      <div className="flex items-end justify-end gap-2">
        <Button variant="outline" onClick={onCancel}>
          Cancelar
        </Button>
        <Button
          disabled={saving || !draft.name.trim() || !unitsValid}
          onClick={onSave}
        >
          {saving ? "Salvando…" : "Salvar item"}
        </Button>
      </div>
    </div>
  );
}

function itemToDraft(item: ItemResponse): ItemDraft {
  return {
    name: item.name,
    parentItemId: item.parent_item_id ?? "",
    unitName: item.unit_name ?? "",
    quantityScale: item.quantity_scale ?? "",
    status: item.status,
  };
}

function describePrice(price: PriceVersionResponse): string {
  if (price.pricing_model === "unit") {
    return `A cada ${formatUnits(price.unit_block_size)} unidade(s), cobrar ${formatUnits(price.credit_units)} créditos`;
  }
  return `${price.tiers.length} faixa(s) de consumo acumulado`;
}
