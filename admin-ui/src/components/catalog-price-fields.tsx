import { Plus } from "lucide-react";
import type { PriceTierDraft } from "@/lib/catalog-product";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export function PriceModelSelect({
  id,
  value,
  disabled,
  onChange,
}: {
  id: string;
  value: "unit" | "tiered";
  disabled?: boolean;
  onChange: (value: "unit" | "tiered") => void;
}) {
  return (
    <Select
      items={[{ label: "Preço por bloco", value: "unit" }, { label: "Preço por faixas", value: "tiered" }]}
      value={value}
      onValueChange={(selected) => onChange(selected === "tiered" ? "tiered" : "unit")}
      disabled={disabled}
    >
      <SelectTrigger id={id} className="w-full"><SelectValue /></SelectTrigger>
      <SelectContent><SelectGroup><SelectItem value="unit">Preço por bloco</SelectItem><SelectItem value="tiered">Preço por faixas</SelectItem></SelectGroup></SelectContent>
    </Select>
  );
}

export function CatalogTierEditor({
  id,
  tiers,
  disabled = false,
  onChange,
}: {
  id: string;
  tiers: PriceTierDraft[];
  disabled?: boolean;
  onChange: (tiers: PriceTierDraft[]) => void;
}) {
  const updateTier = (index: number, key: keyof PriceTierDraft, value: string) => {
    onChange(tiers.map((tier, current) => current === index ? { ...tier, [key]: value } : tier));
  };
  return (
    <Field className="md:col-span-2">
      <FieldLabel>Faixas de consumo acumulado</FieldLabel>
      <FieldGroup>
        {tiers.map((tier, index) => (
          <div key={index} className="grid gap-3 rounded-md border p-3 sm:grid-cols-4">
            <TierInput id={`${id}-${index}-from`} label="Início" value={tier.from} disabled={disabled} onChange={(value) => updateTier(index, "from", value)} />
            <TierInput id={`${id}-${index}-to`} label="Fim (vazio na última)" value={tier.to} disabled={disabled} onChange={(value) => updateTier(index, "to", value)} />
            <TierInput id={`${id}-${index}-block`} label="Unidades por bloco" value={tier.block} disabled={disabled} onChange={(value) => updateTier(index, "block", value)} />
            <TierInput id={`${id}-${index}-credits`} label="Créditos por bloco" value={tier.credits} disabled={disabled} onChange={(value) => updateTier(index, "credits", value)} />
            {tiers.length > 1 && <Button type="button" variant="ghost" disabled={disabled} onClick={() => onChange(tiers.filter((_, current) => current !== index))}>Remover faixa</Button>}
          </div>
        ))}
        <Button type="button" variant="outline" disabled={disabled} onClick={() => onChange([...tiers, { from: tiers.at(-1)?.to ?? "", to: "", block: "1", credits: "1" }])}>
          <Plus data-icon="inline-start" /> Adicionar faixa
        </Button>
      </FieldGroup>
    </Field>
  );
}

function TierInput({
  id,
  label,
  value,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input id={id} inputMode="numeric" value={value} onChange={(event) => onChange(event.target.value)} disabled={disabled} />
    </Field>
  );
}
