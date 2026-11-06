import { useState, type FormEvent } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  createTieredPrice,
  createUnitPrice,
  getItem,
  getProduct,
  listAllCatalogEntries,
} from "@/api/catalog-api";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import {
  LinkedCreateShell,
  RecordPicker,
} from "@/components/catalog-record-picker";
import { entriesWithIds } from "@/lib/catalog-entry";
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
import {
  CatalogTierEditor,
  PriceModelSelect,
} from "@/components/catalog-price-fields";
import {
  newCatalogItemDraft,
  validateCatalogProductDraft,
  type PriceTierDraft,
} from "@/lib/catalog-product";

export function CatalogPriceCreateForm() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [productId, setProductId] = useState("");
  const [itemId, setItemId] = useState(params.get("item_id") ?? "");
  const [model, setModel] = useState<"unit" | "tiered">("unit");
  const [block, setBlock] = useState("1");
  const [credits, setCredits] = useState("");
  const [tiers, setTiers] = useState<PriceTierDraft[]>([
    { from: "0", to: "", block: "1", credits: "1" },
  ]);
  const [frequency, setFrequency] = useState("MONTHLY");
  const [interval, setInterval] = useState("1");
  const [anchor, setAnchor] = useState("");
  const [effectiveFrom, setEffectiveFrom] = useState("");
  const [effectiveUntil, setEffectiveUntil] = useState("");
  const [error, setError] = useState<Error | null>(null);
  const [pending, setPending] = useState(false);
  const products = useQuery({
    queryKey: ["catalog-all", "products"],
    queryFn: () => listAllCatalogEntries("products"),
  });
  const allItems = useQuery({
    queryKey: ["catalog-all", "items"],
    queryFn: () => listAllCatalogEntries("items"),
  });
  const selectedItem = allItems.data?.find((item) => item.id === itemId);
  const linkedProductId = productId || selectedItem?.parent_id || "";
  const product = useQuery({
    queryKey: ["product-detail", linkedProductId],
    queryFn: () => getProduct(linkedProductId),
    enabled: !!linkedProductId,
  });
  const productItems = useQuery({
    queryKey: ["catalog-all", "items", linkedProductId],
    queryFn: () => listAllCatalogEntries("items", linkedProductId),
    enabled: !!linkedProductId,
  });
  const selectedItemDetails = useQuery({
    queryKey: ["item-detail", itemId],
    queryFn: () => getItem(itemId),
    enabled: !!itemId,
  });
  const productOptions = entriesWithIds(products.data ?? []);
  const itemOptions = entriesWithIds(productItems.data ?? []);
  const itemProduct = selectedItem
    ? products.data?.find((entry) => entry.id === selectedItem.parent_id)
    : undefined;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setPending(true);
    setError(null);
    try {
      if (!itemId || product.data?.usage_model !== "CREDIT_METERED")
        throw new Error("Selecione um item de consumo de um produto válido.");
      if (
        model === "unit" &&
        (!/^\d+$/.test(block) ||
          BigInt(block) <= 0n ||
          !/^\d+$/.test(credits) ||
          BigInt(credits) <= 0n)
      )
        throw new Error(
          "Informe quantidades inteiras positivas para cobrança e créditos.",
        );
      if (model === "tiered") {
        const tierDraft = newCatalogItemDraft(
          selectedItemDetails.data?.name ?? "Item",
        );
        tierDraft.pricingModel = "tiered";
        tierDraft.tiers = tiers;
        tierDraft.effectiveFrom = effectiveFrom;
        tierDraft.effectiveUntil = effectiveUntil;
        tierDraft.accumulationAnchorAt = anchor;
        tierDraft.accumulationRecurrenceRule = `FREQ=${frequency};INTERVAL=${interval}`;
        const validation = validateCatalogProductDraft(
          {
            name: "Preço",
            description: "",
            usageModel: "CREDIT_METERED",
            items: [tierDraft],
          },
          false,
        );
        if (validation) throw new Error(validation);
      }
      const start = effectiveFrom
        ? new Date(effectiveFrom).toISOString()
        : new Date().toISOString();
      const end = effectiveUntil
        ? new Date(effectiveUntil).toISOString()
        : null;
      const result =
        model === "unit"
          ? await createUnitPrice(itemId, {
              unit_block_size: block,
              credit_units: credits,
              effective_from: start,
              effective_until: end,
            })
          : await createTieredPrice(itemId, {
              effective_from: start,
              effective_until: end,
              accumulation_cycle: anchor
                ? {
                    anchor_at: new Date(anchor).toISOString(),
                    recurrence_rule: `FREQ=${frequency};INTERVAL=${interval}`,
                  }
                : null,
              tiers: tiers.map((tier) => ({
                from_accumulated_units: tier.from,
                to_accumulated_units: tier.to || null,
                unit_block_size: tier.block,
                credit_units: tier.credits,
              })),
            });
      navigate(`/catalog/prices/${result.price_version_id}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason : new Error(String(reason)));
    } finally {
      setPending(false);
    }
  }

  return (
    <LinkedCreateShell
      kind="prices"
      onSubmit={submit}
      pending={pending}
      error={error}
    >
      {products.isLoading ? (
        <QueryLoading />
      ) : products.error ? (
        <QueryError error={products.error} />
      ) : (
        <RecordPicker
          label="Produto"
          entries={productOptions}
          value={linkedProductId}
          onChange={(id) => {
            setProductId(id);
            setItemId("");
          }}
          required
        />
      )}
      {product.data?.usage_model === "ENTITLEMENT_ONLY" && (
        <p role="alert" className="text-sm text-destructive">
          Este produto é de acesso por assinatura e não aceita preço de consumo.
        </p>
      )}
      {productItems.isLoading && linkedProductId && <QueryLoading />}
      {productItems.error && <QueryError error={productItems.error} />}
      {productItems.data && (
        <RecordPicker
          label="Item"
          entries={itemOptions}
          value={itemId}
          onChange={setItemId}
          required
        />
      )}
      {selectedItemDetails.data?.unit_name && (
        <FieldDescription>
          Unidade: {selectedItemDetails.data.unit_name}
        </FieldDescription>
      )}
      {itemProduct && (
        <FieldDescription>
          Produto selecionado: {itemProduct.name}
        </FieldDescription>
      )}
      {itemId && (
        <>
          <Field>
            <FieldLabel>Modelo de preço</FieldLabel>
            <PriceModelSelect
              id="price-model"
              value={model}
              onChange={setModel}
            />
          </Field>
          {model === "unit" ? (
            <FieldGroup className="grid gap-4 md:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="price-credits">
                  Créditos por cobrança
                </FieldLabel>
                <Input
                  id="price-credits"
                  inputMode="numeric"
                  value={credits}
                  onChange={(event) => setCredits(event.target.value)}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="price-block">
                  Quantidade por cobrança
                </FieldLabel>
                <Input
                  id="price-block"
                  inputMode="numeric"
                  value={block}
                  onChange={(event) => setBlock(event.target.value)}
                  required
                />
              </Field>
            </FieldGroup>
          ) : (
            <>
              <details>
                <summary className="cursor-pointer text-sm font-medium">
                  Acumulação
                </summary>
                <FieldGroup className="mt-4">
                  <Field>
                    <FieldLabel htmlFor="price-anchor">
                      Início do ciclo (opcional)
                    </FieldLabel>
                    <Input
                      id="price-anchor"
                      type="datetime-local"
                      value={anchor}
                      onChange={(event) => setAnchor(event.target.value)}
                    />
                  </Field>
                  {anchor && (
                    <>
                      <Field>
                        <FieldLabel htmlFor="price-frequency">
                          Frequência
                        </FieldLabel>
                        <Select
                          value={frequency}
                          onValueChange={(value) =>
                            setFrequency(value ?? "MONTHLY")
                          }
                        >
                          <SelectTrigger
                            id="price-frequency"
                            className="w-full"
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              {[
                                ["DAILY", "Diária"],
                                ["WEEKLY", "Semanal"],
                                ["MONTHLY", "Mensal"],
                                ["YEARLY", "Anual"],
                              ].map(([value, label]) => (
                                <SelectItem key={value} value={value}>
                                  {label}
                                </SelectItem>
                              ))}
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field>
                        <FieldLabel htmlFor="price-interval">
                          A cada quantas frequências?
                        </FieldLabel>
                        <Input
                          id="price-interval"
                          inputMode="numeric"
                          value={interval}
                          onChange={(event) => setInterval(event.target.value)}
                          required
                        />
                      </Field>
                    </>
                  )}
                </FieldGroup>
              </details>
              <CatalogTierEditor
                id="price-tiers"
                tiers={tiers}
                onChange={(next) => {
                  const linked = next.map((tier, index) => ({
                    ...tier,
                    from: index === 0 ? "0" : (next[index - 1]?.to ?? ""),
                  }));
                  setTiers(linked);
                }}
              />
            </>
          )}
          <details>
            <summary className="cursor-pointer text-sm font-medium">
              Vigência
            </summary>
            <FieldGroup className="mt-4">
              <Field>
                <FieldLabel htmlFor="price-from">Início (opcional)</FieldLabel>
                <Input
                  id="price-from"
                  type="datetime-local"
                  value={effectiveFrom}
                  onChange={(event) => setEffectiveFrom(event.target.value)}
                />
                <FieldDescription>
                  Vazio significa a partir do envio.
                </FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="price-until">
                  Término (opcional)
                </FieldLabel>
                <Input
                  id="price-until"
                  type="datetime-local"
                  value={effectiveUntil}
                  onChange={(event) => setEffectiveUntil(event.target.value)}
                />
              </Field>
            </FieldGroup>
          </details>
        </>
      )}
    </LinkedCreateShell>
  );
}
