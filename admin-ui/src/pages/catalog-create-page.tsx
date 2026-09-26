import { useState, type FormEvent } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import {
  createAdmissionPolicy,
  createItem,
  createOnDemandPlan,
  createProduct,
  createSubscription,
  createSubscriptionPlan,
  createTieredPrice,
  createUnitPrice,
} from "@/api/catalog-api";
import { QueryError } from "@/components/query-feedback";
import { CatalogTierEditor, PriceModelSelect } from "@/components/catalog-price-fields";
import { CatalogProductCreatePage } from "@/pages/catalog-product-create-page";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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
  catalogKinds,
  parseCatalogKind,
  type CatalogKind,
} from "@/lib/catalog-kinds";

interface FieldSpec {
  name: string;
  label: string;
  description?: string;
  required?: boolean;
  type?: string;
  placeholder?: string;
  options?: string[];
  optionLabels?: Record<string, string>;
}

const fields: Record<CatalogKind, FieldSpec[]> = {
  products: [
    { name: "name", label: "Nome", required: true },
    { name: "description", label: "Descrição" },
    {
      name: "usage_model",
      label: "Como este produto será usado?",
      description:
        "Na versão atual, somente produtos com consumo cobrado em créditos podem ser publicados.",
      required: true,
      options: ["CREDIT_METERED", "ENTITLEMENT_ONLY"],
      optionLabels: {
        CREDIT_METERED: "Consumo cobrado em créditos",
        ENTITLEMENT_ONLY: "Acesso por assinatura, sem cobrança por consumo",
      },
    },
  ],
  items: [
    { name: "product_id", label: "Produto ID", required: true },
    { name: "name", label: "Nome", required: true },
    { name: "parent_item_id", label: "Item pai ID" },
    { name: "unit_name", label: "Nome da unidade" },
    {
      name: "quantity_scale",
      label: "Escala de quantidade",
      placeholder: "String decimal; junto com nome da unidade",
    },
  ],
  prices: [
    { name: "item_id", label: "Item ID", required: true },
    {
      name: "effective_from",
      label: "Vigência inicial",
      required: true,
      type: "datetime-local",
    },
    {
      name: "effective_until",
      label: "Vigência final",
      type: "datetime-local",
    },
  ],
  subscriptions: [
    { name: "name", label: "Nome", required: true },
    {
      name: "subscription_model",
      label: "Modelo",
      required: true,
      options: ["CREDIT_STRICT", "CREDIT_FLEXIBLE", "ENTITLEMENT_ONLY"],
    },
  ],
  plans: [
    { name: "subscription_id", label: "Assinatura ID", required: true },
    { name: "name", label: "Nome", required: true },
    {
      name: "commercial_model",
      label: "Modelo comercial",
      required: true,
      options: ["FREE", "PAID"],
    },
    {
      name: "price_amount_minor",
      label: "Preço em unidades menores; obrigatório para PAID",
    },
    {
      name: "currency",
      label: "Moeda; obrigatório para PAID",
      placeholder: "BRL",
    },
    {
      name: "recurrence",
      label: "Recorrência",
      required: true,
      options: ["NONE", "WEEKLY", "MONTHLY", "QUARTERLY", "ANNUALLY"],
    },
    {
      name: "admission_policy",
      label: "Admissão",
      required: true,
      options: ["OPEN", "APPROVAL_REQUIRED"],
    },
    { name: "admission_policy_version_id", label: "Versão da política ID" },
    {
      name: "granted_credit_units",
      label: "Créditos concedidos",
      required: true,
      placeholder: "0",
    },
    {
      name: "product_ids",
      label: "Produtos ID",
      placeholder: "UUIDs separados por vírgula",
    },
  ],
  "on-demand": [
    { name: "subscription_id", label: "Assinatura ID", required: true },
    { name: "name", label: "Nome", required: true },
    {
      name: "price_amount_minor",
      label: "Preço em unidades menores",
      required: true,
    },
    { name: "currency", label: "Moeda", required: true, placeholder: "BRL" },
    { name: "credit_units", label: "Unidades de crédito", required: true },
  ],
  policies: [
    { name: "policy_id", label: "Política ID", required: true },
    { name: "version", label: "Versão", required: true, type: "number" },
    {
      name: "required_facts",
      label: "Fatos exigidos",
      required: true,
      options: ["EMAIL_VERIFIED", "IDENTITY_VERIFIED"],
    },
  ],
};

export function CatalogCreatePage() {
  const { kind: rawKind } = useParams();
  const [searchParams] = useSearchParams();
  const kind = parseCatalogKind(rawKind);
  const navigate = useNavigate();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  if (!kind) return <p>Tipo de catálogo desconhecido.</p>;
  if (kind === "products") return <CatalogProductCreatePage />;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!kind) return;
    setPending(true);
    setError(null);
    try {
      const createdId = await createEntry(
        kind,
        new FormData(event.currentTarget),
      );
      navigate(`/catalog/${kind}/${createdId}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason : new Error(String(reason)));
    } finally {
      setPending(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link
          className="text-sm text-primary hover:underline"
          to={`/catalog/${kind}`}
        >
          ← {catalogKinds[kind]}
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">
          Criar {catalogKinds[kind].toLowerCase()}
        </h1>
        <p className="text-muted-foreground">
          Confira os valores antes de enviar. Preços são criados como rascunho e
          publicados no detalhe.
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>Nova oferta</CardTitle>
          <CardDescription>
            IDs referem-se aos registros da mesma API.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submit} className="flex flex-col gap-5">
            <FieldGroup className="grid gap-4 md:grid-cols-2">
              {fields[kind].map((field) => (
                <Field key={field.name}>
                  <FieldLabel htmlFor={field.name}>{field.label}</FieldLabel>
                  {field.options ? (
                    <Select
                      items={field.options.map((option) => ({
                        label: field.optionLabels?.[option] ?? option,
                        value: option,
                      }))}
                      name={field.name}
                      required={field.required}
                      defaultValue={field.options[0]}
                    >
                      <SelectTrigger id={field.name} className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {field.options.map((option) => (
                            <SelectItem key={option} value={option}>
                              {field.optionLabels?.[option] ?? option}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  ) : (
                    <Input
                      id={field.name}
                      name={field.name}
                      type={field.type ?? "text"}
                      required={field.required}
                      placeholder={field.placeholder}
                      defaultValue={
                        field.name === "product_id"
                          ? (searchParams.get("product_id") ?? undefined)
                          : field.name === "item_id"
                            ? (searchParams.get("item_id") ?? undefined)
                            : undefined
                      }
                    />
                  )}
                  {field.description && (
                    <FieldDescription>{field.description}</FieldDescription>
                  )}
                </Field>
              ))}
            </FieldGroup>
            {error && <QueryError error={error} />}
            {kind === "prices" && <PriceConversionFields />}
            <Button type="submit" disabled={pending}>
              {pending ? "Enviando…" : "Criar e conferir"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}

async function createEntry(kind: CatalogKind, form: FormData): Promise<string> {
  const value = (name: string) => String(form.get(name) ?? "").trim();
  const optional = (name: string) => value(name) || null;
  switch (kind) {
    case "products":
      return (
        await createProduct({
          name: value("name"),
          description: optional("description"),
          usage_model: value("usage_model") as
            "CREDIT_METERED" | "ENTITLEMENT_ONLY",
        })
      ).product_id;
    case "items":
      return (
        await createItem(value("product_id"), {
          name: value("name"),
          parent_item_id: optional("parent_item_id"),
          unit_name: optional("unit_name"),
          quantity_scale: optional("quantity_scale"),
        })
      ).item_id;
    case "prices": {
      const dates = {
        effective_from: new Date(value("effective_from")).toISOString(),
        effective_until: optional("effective_until")
          ? new Date(value("effective_until")).toISOString()
          : null,
      };
      if (value("pricing_model") === "unit")
        return (
          await createUnitPrice(value("item_id"), {
            unit_block_size: value("unit_block_size"),
            credit_units: value("credit_units"),
            ...dates,
          })
        ).price_version_id;
      const tiers = JSON.parse(value("tiers")) as {
        from_accumulated_units: string;
        to_accumulated_units: string | null;
        unit_block_size: string;
        credit_units: string;
      }[];
      const anchor = optional("accumulation_anchor_at");
      return (
        await createTieredPrice(value("item_id"), {
          ...dates,
          accumulation_cycle: anchor
            ? {
                anchor_at: new Date(anchor).toISOString(),
                recurrence_rule: value("accumulation_recurrence_rule"),
              }
            : null,
          tiers,
        })
      ).price_version_id;
    }
    case "subscriptions":
      return (
        await createSubscription({
          name: value("name"),
          subscription_model: value("subscription_model") as
            "CREDIT_STRICT" | "CREDIT_FLEXIBLE" | "ENTITLEMENT_ONLY",
        })
      ).subscription_id;
    case "plans": {
      const paid = value("commercial_model") === "PAID";
      const amount = optional("price_amount_minor")
        ? Number(value("price_amount_minor"))
        : null;
      if (
        paid &&
        (!Number.isSafeInteger(amount) || amount === null || amount <= 0)
      )
        throw new Error(
          "Preço pago deve ser um inteiro positivo em unidades menores.",
        );
      const result = await createSubscriptionPlan(value("subscription_id"), {
        name: value("name"),
        commercial_model: paid ? "PAID" : "FREE",
        price_amount_minor: paid ? amount : null,
        currency: paid ? value("currency").toUpperCase() : null,
        recurrence: value("recurrence") as
          "NONE" | "WEEKLY" | "MONTHLY" | "QUARTERLY" | "ANNUALLY",
        admission_policy: value("admission_policy") as
          "OPEN" | "APPROVAL_REQUIRED",
        admission_policy_version_id: optional("admission_policy_version_id"),
        accepted_payment_methods: paid ? ["CARD"] : [],
        granted_credit_units: value("granted_credit_units"),
        product_ids: value("product_ids")
          .split(",")
          .map((id) => id.trim())
          .filter(Boolean),
      });
      return result.plan_version_id;
    }
    case "on-demand":
      return (
        await createOnDemandPlan(value("subscription_id"), {
          name: value("name"),
          price_amount_minor: Number(value("price_amount_minor")),
          currency: value("currency").toUpperCase(),
          credit_units: value("credit_units"),
        })
      ).on_demand_plan_id;
    case "policies":
      return (
        await createAdmissionPolicy({
          policy_id: value("policy_id"),
          version: Number(value("version")),
          required_facts: [
            value("required_facts") as "EMAIL_VERIFIED" | "IDENTITY_VERIFIED",
          ],
        })
      ).policy_version_id;
  }
}

function PriceConversionFields() {
  const [model, setModel] = useState<"unit" | "tiered">("unit");
  const [tiers, setTiers] = useState([
    { from: "0", to: "", block: "1", credits: "1" },
  ]);
  return (
    <FieldGroup className="flex flex-col gap-4">
      <Field>
        <FieldLabel htmlFor="pricing_model_select">Modelo de preço</FieldLabel>
        <PriceModelSelect id="pricing_model_select" value={model} onChange={setModel} />
        <input type="hidden" name="pricing_model" value={model} />
      </Field>
      {model === "unit" ? (
        <div className="grid gap-4 md:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="unit_block_size">Tamanho do bloco</FieldLabel>
            <Input id="unit_block_size" name="unit_block_size" required inputMode="numeric" />
          </Field>
          <Field>
            <FieldLabel htmlFor="credit_units">Créditos por bloco</FieldLabel>
            <Input id="credit_units" name="credit_units" required inputMode="numeric" />
          </Field>
        </div>
      ) : (
        <>
          <div className="grid gap-4 md:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="accumulation_anchor_at">Âncora do ciclo (opcional)</FieldLabel>
              <Input id="accumulation_anchor_at" name="accumulation_anchor_at" type="datetime-local" />
            </Field>
            <Field>
              <FieldLabel htmlFor="accumulation_recurrence_rule">Regra de recorrência</FieldLabel>
              <Input id="accumulation_recurrence_rule" name="accumulation_recurrence_rule" placeholder="FREQ=MONTHLY;INTERVAL=1" />
            </Field>
          </div>
          <CatalogTierEditor id="catalog-price" tiers={tiers} onChange={setTiers} />
          <input
            type="hidden"
            name="tiers"
            value={JSON.stringify(tiers.map((tier) => ({
              from_accumulated_units: tier.from,
              to_accumulated_units: tier.to || null,
              unit_block_size: tier.block,
              credit_units: tier.credits,
            })))}
          />
        </>
      )}
    </FieldGroup>
  );
}
