import { Trash2 } from "lucide-react";
import {
  CatalogTierEditor,
  PriceModelSelect,
} from "@/components/catalog-price-fields";
import {
  catalogUnitOptions,
  knownCatalogUnit,
  type CatalogProductItemDraft,
} from "@/lib/catalog-product";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export function ProductItemCard({
  index,
  item,
  items,
  suggestedName,
  locked,
  onChange,
  onRemove,
}: {
  index: number;
  item: CatalogProductItemDraft;
  items: CatalogProductItemDraft[];
  suggestedName: string;
  locked: boolean;
  onChange: (update: Partial<CatalogProductItemDraft>) => void;
  onRemove: () => void;
}) {
  const id = `item-${item.draftId}`;
  return (
    <Card size="sm">
      <CardHeader>
        <div className="flex items-center justify-between gap-3">
          <CardTitle>
            {items.length > 1
              ? item.name || `Item ${index + 1}`
              : "Consumo e preço"}
          </CardTitle>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={locked}
            onClick={onRemove}
          >
            <Trash2 data-icon="inline-start" /> Remover
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        {items.length > 1 && (
          <Field>
            <FieldLabel htmlFor={`${id}-name`}>Nome do consumo</FieldLabel>
            <Input
              id={`${id}-name`}
              value={item.name}
              onChange={(event) => onChange({ name: event.target.value })}
              placeholder={suggestedName || "Ex.: requisições"}
              disabled={locked}
            />
          </Field>
        )}
        <FieldGroup className="grid gap-4 md:grid-cols-2">
          <Field>
            <FieldLabel htmlFor={`${id}-unit`}>Unidade</FieldLabel>
            <Select
              value={knownCatalogUnit(item.unitName) ? item.unitName : "custom"}
              onValueChange={(value) =>
                onChange({
                  unitName: value === "custom" ? "" : (value ?? "unidade"),
                })
              }
              disabled={locked}
            >
              <SelectTrigger id={`${id}-unit`} className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {catalogUnitOptions.map((unit) => (
                    <SelectItem key={unit} value={unit}>
                      {unit}
                    </SelectItem>
                  ))}
                  <SelectItem value="custom">Outra unidade…</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          {!knownCatalogUnit(item.unitName) && (
            <Field>
              <FieldLabel htmlFor={`${id}-custom-unit`}>
                Nome da unidade
              </FieldLabel>
              <Input
                id={`${id}-custom-unit`}
                value={item.unitName}
                onChange={(event) => onChange({ unitName: event.target.value })}
                placeholder="Ex.: imagem"
                disabled={locked}
                required
              />
            </Field>
          )}
          {item.pricingModel === "unit" ? (
            <Field>
              <FieldLabel htmlFor={`${id}-credits`}>
                Créditos por cobrança
              </FieldLabel>
              <Input
                id={`${id}-credits`}
                inputMode="numeric"
                value={item.creditUnits}
                onChange={(event) =>
                  onChange({ creditUnits: event.target.value })
                }
                placeholder="Ex.: 5"
                disabled={locked}
                required
              />
              <FieldDescription>
                A cada {item.unitBlockSize || "1"} {item.unitName || "unidade"},
                cobrar {item.creditUnits || "—"} créditos.
              </FieldDescription>
            </Field>
          ) : (
            <Field>
              <FieldLabel htmlFor={`${id}-anchor`}>
                Início do ciclo (opcional)
              </FieldLabel>
              <Input
                id={`${id}-anchor`}
                type="datetime-local"
                value={item.accumulationAnchorAt}
                onChange={(event) =>
                  onChange({ accumulationAnchorAt: event.target.value })
                }
                disabled={locked}
              />
              {item.accumulationAnchorAt && (
                <FieldDescription>
                  Regra: {item.accumulationRecurrenceRule}
                </FieldDescription>
              )}
            </Field>
          )}
          {items.length > 1 && (
            <ParentItemField
              item={item}
              items={items}
              id={id}
              locked={locked}
              onChange={onChange}
            />
          )}
        </FieldGroup>
        <details>
          <summary className="cursor-pointer text-sm font-medium">
            Configurações avançadas
          </summary>
          <FieldGroup className="mt-4 grid gap-4 md:grid-cols-2">
            {items.length === 1 && (
              <Field>
                <FieldLabel htmlFor={`${id}-name`}>
                  Nome do item (opcional)
                </FieldLabel>
                <Input
                  id={`${id}-name`}
                  value={item.name}
                  onChange={(event) => onChange({ name: event.target.value })}
                  placeholder={suggestedName || "Nome do produto"}
                  disabled={locked}
                />
                <FieldDescription>
                  Por padrão, o item usa o nome do produto.
                </FieldDescription>
              </Field>
            )}
            <Field>
              <FieldLabel htmlFor={`${id}-block`}>
                Quantidade por cobrança
              </FieldLabel>
              <Input
                id={`${id}-block`}
                inputMode="numeric"
                value={
                  item.pricingModel === "unit"
                    ? item.unitBlockSize
                    : "Por faixa"
                }
                onChange={(event) =>
                  onChange({ unitBlockSize: event.target.value })
                }
                disabled={locked || item.pricingModel === "tiered"}
              />
              <FieldDescription>
                Ex.: cobrar a cada 100 requisições.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-scale`}>Escala da unidade</FieldLabel>
              <Input
                id={`${id}-scale`}
                inputMode="numeric"
                value={item.quantityScale}
                onChange={(event) =>
                  onChange({ quantityScale: event.target.value })
                }
                disabled={locked}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-pricing-model`}>
                Modelo de preço
              </FieldLabel>
              <PriceModelSelect
                id={`${id}-pricing-model`}
                value={item.pricingModel}
                disabled={locked}
                onChange={(pricingModel) => onChange({ pricingModel })}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-from`}>
                Vigência inicial (opcional)
              </FieldLabel>
              <Input
                id={`${id}-from`}
                type="datetime-local"
                value={item.effectiveFrom}
                onChange={(event) =>
                  onChange({ effectiveFrom: event.target.value })
                }
                disabled={locked}
              />
              <FieldDescription>
                Vazio significa a partir do envio.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-until`}>
                Vigência final (opcional)
              </FieldLabel>
              <Input
                id={`${id}-until`}
                type="datetime-local"
                value={item.effectiveUntil}
                onChange={(event) =>
                  onChange({ effectiveUntil: event.target.value })
                }
                disabled={locked}
              />
            </Field>
            {item.pricingModel === "tiered" && (
              <CatalogTierEditor
                id={item.draftId}
                tiers={item.tiers}
                disabled={locked}
                onChange={(tiers) => onChange({ tiers })}
              />
            )}
          </FieldGroup>
        </details>
      </CardContent>
    </Card>
  );
}

function ParentItemField({
  item,
  items,
  id,
  locked,
  onChange,
}: {
  item: CatalogProductItemDraft;
  items: CatalogProductItemDraft[];
  id: string;
  locked: boolean;
  onChange: (update: Partial<CatalogProductItemDraft>) => void;
}) {
  const parents = items.filter(
    (candidate) => candidate.draftId !== item.draftId,
  );
  return (
    <Field>
      <FieldLabel htmlFor={`${id}-parent`}>Item pai (opcional)</FieldLabel>
      <Select
        items={[
          { label: "Sem item pai", value: "" },
          ...parents.map((candidate) => ({
            label: candidate.name || "Novo item",
            value: candidate.draftId,
          })),
        ]}
        value={item.parentDraftId}
        onValueChange={(value) => onChange({ parentDraftId: value ?? "" })}
        disabled={locked}
      >
        <SelectTrigger id={`${id}-parent`} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            <SelectItem value="">Sem item pai</SelectItem>
            {parents.map((candidate) => (
              <SelectItem key={candidate.draftId} value={candidate.draftId}>
                {candidate.name || "Novo item"}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  );
}
