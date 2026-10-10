import { useMemo, useState, type FormEvent } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  createItem,
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
import { catalogUnitOptions, knownCatalogUnit } from "@/lib/catalog-product";

export function CatalogItemCreateForm() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [productId, setProductId] = useState(params.get("product_id") ?? "");
  const [itemName, setItemName] = useState("");
  const [unit, setUnit] = useState("unidade");
  const [customUnit, setCustomUnit] = useState("");
  const [scale, setScale] = useState("1");
  const [parentId, setParentId] = useState("");
  const [error, setError] = useState<Error | null>(null);
  const [pending, setPending] = useState(false);
  const products = useQuery({
    queryKey: ["catalog-all", "products"],
    queryFn: () => listAllCatalogEntries("products"),
  });
  const product = useQuery({
    queryKey: ["product-detail", productId],
    queryFn: () => getProduct(productId),
    enabled: !!productId,
  });
  const items = useQuery({
    queryKey: ["catalog-all", "items", productId],
    queryFn: () => listAllCatalogEntries("items", productId),
    enabled: !!productId,
  });
  const productOptions = useMemo(
    () => entriesWithIds(products.data ?? []),
    [products.data],
  );

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setPending(true);
    setError(null);
    try {
      if (!productId || !itemName.trim())
        throw new Error("Selecione um produto e informe o nome do item.");
      const unitName =
        product.data?.usage_model === "ENTITLEMENT_ONLY"
          ? null
          : unit === "custom"
            ? customUnit.trim()
            : unit;
      if (product.data?.usage_model === "CREDIT_METERED" && !unitName)
        throw new Error("Informe o nome da unidade.");
      if (unitName && (!/^\d+$/.test(scale) || BigInt(scale) <= 0n))
        throw new Error("A escala deve ser um inteiro positivo.");
      const result = await createItem(productId, {
        name: itemName.trim(),
        parent_item_id: parentId || null,
        unit_name: unitName,
        quantity_scale: unitName ? scale : null,
      });
      navigate(`/catalog/items/${result.item_id}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason : new Error(String(reason)));
    } finally {
      setPending(false);
    }
  }

  return (
    <LinkedCreateShell
      kind="items"
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
          value={productId}
          onChange={(id) => {
            setProductId(id);
            setParentId("");
          }}
          required
        />
      )}
      {product.isLoading && productId && <QueryLoading />}
      {product.error && <QueryError error={product.error} />}
      {product.data && (
        <>
          <Field>
            <FieldLabel htmlFor="item-name">Nome do item</FieldLabel>
            <Input
              id="item-name"
              value={itemName}
              onChange={(event) => setItemName(event.target.value)}
              required
              placeholder="Ex.: requisições"
            />
          </Field>
          {product.data.usage_model === "CREDIT_METERED" && (
            <>
              <Field>
                <FieldLabel htmlFor="item-unit">Unidade de consumo</FieldLabel>
                <Select
                  value={knownCatalogUnit(unit) ? unit : "custom"}
                  onValueChange={(value) => setUnit(value ?? "unidade")}
                >
                  <SelectTrigger id="item-unit" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {catalogUnitOptions.map((option) => (
                        <SelectItem key={option} value={option}>
                          {option}
                        </SelectItem>
                      ))}
                      <SelectItem value="custom">Outra unidade…</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
                {unit === "custom" && (
                  <Input
                    aria-label="Nome da unidade"
                    value={customUnit}
                    onChange={(event) => setCustomUnit(event.target.value)}
                    required
                    placeholder="Ex.: imagem"
                  />
                )}
              </Field>
              <details>
                <summary className="cursor-pointer text-sm font-medium">
                  Configurações avançadas
                </summary>
                <FieldGroup className="mt-4">
                  <Field>
                    <FieldLabel htmlFor="item-scale">
                      Escala da unidade
                    </FieldLabel>
                    <Input
                      id="item-scale"
                      inputMode="numeric"
                      value={scale}
                      onChange={(event) => setScale(event.target.value)}
                    />
                    <FieldDescription>
                      Quantidade da unidade atômica usada pela integração.
                    </FieldDescription>
                  </Field>
                </FieldGroup>
              </details>
            </>
          )}
          <details>
            <summary className="cursor-pointer text-sm font-medium">
              Vínculo avançado
            </summary>
            {items.isLoading ? (
              <QueryLoading />
            ) : items.error ? (
              <QueryError error={items.error} />
            ) : (
              <RecordPicker
                label="Item pai (opcional)"
                entries={entriesWithIds(items.data ?? [])}
                value={parentId}
                onChange={setParentId}
              />
            )}
          </details>
        </>
      )}
    </LinkedCreateShell>
  );
}
