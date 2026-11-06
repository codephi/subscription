import { useMutation } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { createCoupon, createVoucher } from "@/api/promotions-api";
import { QueryError } from "@/components/query-feedback";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

export function PromotionCreatePage() {
  const { kind } = useParams();
  const navigate = useNavigate();
  const voucher = kind === "vouchers";
  const [discountKind, setDiscountKind] = useState("PERCENTAGE");
  const create = useMutation({
    mutationFn: (body: Record<string, unknown>) => voucher ? createVoucher(body as Parameters<typeof createVoucher>[0]) : createCoupon(body as Parameters<typeof createCoupon>[0]),
    onSuccess: (promotion) => navigate(`/promotions/${kind ?? "vouchers"}/${promotion.promotion_id}`),
  });
  if (kind !== "vouchers" && kind !== "coupons") return <p>Tipo de promoção desconhecido.</p>;
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const nullable = (name: string) => String(form.get(name) ?? "").trim() || null;
    const limit = (name: string, defaultValue: string | null) => {
      const value = nullable(name) ?? defaultValue;
      return value === null ? null : Number(value);
    };
    const common = {
      code: String(form.get("code") ?? "").trim(), name: String(form.get("name") ?? "").trim(),
      description: nullable("description"), valid_from: dateOrNull(nullable("valid_from")), valid_until: dateOrNull(nullable("valid_until")),
      max_total_uses: limit("max_total_uses", null), max_uses_per_account: limit("max_uses_per_account", "1"),
    };
    if (voucher) {
      create.mutate({ ...common, credit_units: String(form.get("credit_units") ?? "").trim() });
      return;
    }
    const kind = String(form.get("discount_kind") ?? "PERCENTAGE") as "PERCENTAGE" | "FIXED";
    create.mutate({ ...common, discount_kind: kind, discount_value: kind === "PERCENTAGE" ? Math.round(Number(form.get("discount_value")) * 100) : Number(form.get("discount_value")), currency: kind === "FIXED" ? String(form.get("currency") ?? "BRL").trim().toUpperCase() : null,
      applies_to_initial: form.has("applies_to_initial"), applies_to_on_demand: form.has("applies_to_on_demand") });
  }
  return (
    <main className="mx-auto flex max-w-3xl flex-col gap-6">
      <header><h1 className="text-3xl font-semibold tracking-tight">Cadastrar {voucher ? "voucher" : "cupom"}</h1><p className="mt-2 text-sm text-muted-foreground">O código fica fixo. Validade e limites podem ser alterados depois.</p></header>
      <Card><CardHeader><CardTitle>{voucher ? "Créditos do voucher" : "Desconto do cupom"}</CardTitle><CardDescription>Um código pode ser resgatado por até uma utilização em cada account por padrão.</CardDescription></CardHeader>
        <CardContent>
          <form onSubmit={submit} className="flex flex-col gap-5">
            <FieldGroup>
              <Field><FieldLabel htmlFor="promotion-code">Código</FieldLabel><Input id="promotion-code" name="code" required maxLength={64} autoCapitalize="characters" placeholder="BEMVINDO" /><FieldDescription>Letras, números, hífen e sublinhado. Espaços externos e caixa baixa serão normalizados.</FieldDescription></Field>
              <Field><FieldLabel htmlFor="promotion-name">Nome</FieldLabel><Input id="promotion-name" name="name" required maxLength={160} /></Field>
              <Field><FieldLabel htmlFor="promotion-description">Descrição</FieldLabel><Textarea id="promotion-description" name="description" maxLength={1000} /></Field>
              {voucher ? <Field><FieldLabel htmlFor="credit-units">Créditos concedidos por resgate</FieldLabel><Input id="credit-units" name="credit_units" inputMode="numeric" pattern="[0-9]+" required placeholder="100" /><FieldDescription>Unidades inteiras. Os créditos resgatados não expiram.</FieldDescription></Field> : <CouponFields discountKind={discountKind} onKindChange={setDiscountKind} />}
              <div className="grid gap-4 md:grid-cols-2">
                <Field><FieldLabel htmlFor="valid-from">Válido a partir de</FieldLabel><Input id="valid-from" name="valid_from" type="datetime-local" /><FieldDescription>Vazio significa sem data inicial.</FieldDescription></Field>
                <Field><FieldLabel htmlFor="valid-until">Válido até</FieldLabel><Input id="valid-until" name="valid_until" type="datetime-local" /><FieldDescription>Vazio significa sem expiração.</FieldDescription></Field>
                <Field><FieldLabel htmlFor="max-total-uses">Limite total de usos</FieldLabel><Input id="max-total-uses" name="max_total_uses" type="number" min="1" placeholder="Sem limite total" /><FieldDescription>Deixe vazio para permitir usos ilimitados.</FieldDescription></Field>
                <Field><FieldLabel htmlFor="max-account-uses">Usos por account</FieldLabel><Input id="max-account-uses" name="max_uses_per_account" type="number" min="1" defaultValue="1" /><FieldDescription>Defina um número ou deixe vazio para ilimitado.</FieldDescription></Field>
              </div>
            </FieldGroup>
            {create.error && <QueryError error={create.error} />}
            <div className="flex gap-2"><Button type="submit" disabled={create.isPending}>{create.isPending ? "Cadastrando…" : "Cadastrar promoção"}</Button><Button type="button" variant="outline" onClick={() => navigate("/promotions")}>Cancelar</Button></div>
          </form>
        </CardContent>
      </Card>
    </main>
  );
}

function CouponFields({ discountKind, onKindChange }: { discountKind: string; onKindChange: (value: string) => void }) {
  return <>
    <Field><FieldLabel htmlFor="discount-kind">Tipo de desconto</FieldLabel><select id="discount-kind" name="discount_kind" className="h-9 rounded-md border bg-background px-3 text-sm" value={discountKind} onChange={(event) => onKindChange(event.target.value)}><option value="PERCENTAGE">Percentual</option><option value="FIXED">Valor fixo</option></select></Field>
    <div className="grid gap-4 md:grid-cols-2"><Field><FieldLabel htmlFor="discount-value">Desconto{discountKind === "PERCENTAGE" ? " em %" : " em dinheiro"}</FieldLabel><Input id="discount-value" name="discount_value" type="number" min="0.01" max={discountKind === "PERCENTAGE" ? "100" : undefined} step="0.01" required placeholder="10" /><FieldDescription>{discountKind === "PERCENTAGE" ? "Percentual de 0,01% a 100%." : "Valor positivo na moeda selecionada."}</FieldDescription></Field><Field data-disabled={discountKind !== "FIXED"}><FieldLabel htmlFor="discount-currency">Moeda do valor fixo</FieldLabel><Input id="discount-currency" name="currency" defaultValue="BRL" maxLength={3} disabled={discountKind !== "FIXED"} /><FieldDescription>Usada somente para desconto fixo.</FieldDescription></Field></div>
    <FieldSet><FieldLegend>Aplicar em</FieldLegend><FieldGroup><Field orientation="horizontal"><Checkbox id="applies-to-initial" name="applies_to_initial" defaultChecked /><FieldLabel htmlFor="applies-to-initial">Contratação inicial da assinatura</FieldLabel></Field><Field orientation="horizontal"><Checkbox id="applies-to-on-demand" name="applies_to_on_demand" /><FieldLabel htmlFor="applies-to-on-demand">Compra de créditos</FieldLabel></Field></FieldGroup></FieldSet>
  </>;
}

function dateOrNull(value: string | null): string | null { return value ? new Date(value).toISOString() : null; }
