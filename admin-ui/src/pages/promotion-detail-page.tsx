import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, useParams } from "react-router-dom";
import { getPromotion, getPromotionHistory, redeemVoucher, updatePromotion } from "@/api/promotions-api";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { clearPendingKey, pendingKey } from "@/lib/idempotency";
import { formatDate } from "@/lib/format";
import type { components } from "@/api/generated";

export function PromotionDetailPage() {
  const { kind, id } = useParams();
  if ((kind !== "vouchers" && kind !== "coupons") || !id) return <p>Promoção desconhecida.</p>;
  return <PromotionDetail kind={kind} id={id} />;
}

function PromotionDetail({ kind, id }: { kind: "vouchers" | "coupons"; id: string }) {
  const client = useQueryClient();
  const promotion = useQuery({ queryKey: ["promotion", kind, id], queryFn: () => getPromotion(kind, id) });
  const history = useQuery({ queryKey: ["promotion-history", kind, id], queryFn: () => getPromotionHistory(kind, id) });
  const [accountId, setAccountId] = useState("");
  const [transactionId, setTransactionId] = useState("");
  const [redemption, setRedemption] = useState<Awaited<ReturnType<typeof redeemVoucher>> | null>(null);
  const update = useMutation({
    mutationFn: (body: Parameters<typeof updatePromotion>[2]) => updatePromotion(kind, id, body),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["promotion", kind, id] });
      await client.invalidateQueries({ queryKey: ["promotion-history", kind, id] });
      await client.invalidateQueries({ queryKey: ["promotions"] });
    },
  });
  const redeem = useMutation({
    mutationFn: async () => {
      const scope = `voucher:${accountId}`;
      const response = await redeemVoucher(accountId, pendingKey(scope, transactionId), {
        voucher_id: id, code: null, transaction_id: transactionId, description: null,
      });
      clearPendingKey(scope);
      return response;
    },
    onSuccess: async (response) => {
      setRedemption(response);
      await client.invalidateQueries({ queryKey: ["promotion", kind, id] });
      await client.invalidateQueries({ queryKey: ["promotions"] });
      await client.invalidateQueries({ queryKey: ["wallets", accountId] });
      await client.invalidateQueries({ queryKey: ["credits", accountId] });
    },
  });
  function submitEdit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!promotion.data) return;
    const form = new FormData(event.currentTarget);
    const nullable = (name: string) => String(form.get(name) ?? "").trim() || null;
    const integerOrNull = (name: string) => nullable(name) === null ? null : Number(nullable(name));
    update.mutate({
      status: String(form.get("status")) as "ACTIVE" | "DISABLED" | "ARCHIVED",
      valid_from: dateOrNull(nullable("valid_from")), valid_until: dateOrNull(nullable("valid_until")),
      max_total_uses: integerOrNull("max_total_uses"), max_uses_per_account: integerOrNull("max_uses_per_account"),
      expected_version: promotion.data.version, actor_reference: null,
    });
  }
  return (
    <main className="flex flex-col gap-6">
      <header className="flex flex-col gap-2"><Link className="text-sm text-primary hover:underline" to="/promotions">← Promoções</Link><h1 className="text-3xl font-semibold tracking-tight">{promotion.data?.name ?? "Detalhe da promoção"}</h1><p className="font-mono text-sm text-muted-foreground">{promotion.data?.code ?? id}</p></header>
      {promotion.isLoading && <QueryLoading />}{promotion.error && <QueryError error={promotion.error} />}
      {promotion.data && <>
        <Card><CardHeader><CardTitle>Benefício e utilizações</CardTitle><CardDescription>O benefício e o código não podem ser alterados depois do cadastro.</CardDescription></CardHeader><CardContent className="grid gap-4 sm:grid-cols-2">
          <Stat label="Benefício" value={promotion.data.promotion_kind === "VOUCHER" ? `${promotion.data.credit_units} créditos por uso` : couponBenefit(promotion.data)} />
          <Stat label="Estado administrativo" value={promotion.data.status} /><Stat label="Disponibilidade" value={availabilityLabel(promotion.data.availability)} /><Stat label="Usos concluídos" value={`${promotion.data.completed_uses}${promotion.data.max_total_uses ? ` de ${promotion.data.max_total_uses}` : " · sem limite total"}`} /><Stat label="Reservados" value={String(promotion.data.reserved_uses)} />
          <Stat label="Validade" value={`${promotion.data.valid_from ? formatDate(promotion.data.valid_from) : "Sem início"} → ${promotion.data.valid_until ? formatDate(promotion.data.valid_until) : "Sem validade"}`} />
          <Stat label="Usos por account" value={promotion.data.max_uses_per_account === null ? "Ilimitado" : String(promotion.data.max_uses_per_account)} />
        </CardContent></Card>
        <Card><CardHeader><CardTitle>Configuração</CardTitle><CardDescription>A alteração fica registrada no histórico e vale para novas operações.</CardDescription></CardHeader><CardContent><form onSubmit={submitEdit} className="flex flex-col gap-4">
          <FieldGroup><Field><FieldLabel htmlFor="promotion-status">Estado</FieldLabel><select id="promotion-status" name="status" defaultValue={promotion.data.status} disabled={promotion.data.status === "ARCHIVED"} className="h-9 rounded-md border bg-background px-3 text-sm"><option value="ACTIVE">Ativo</option><option value="DISABLED">Desativado</option><option value="ARCHIVED">Arquivado permanentemente</option></select></Field>
            <div className="grid gap-4 md:grid-cols-2"><DateField label="Válido a partir de" name="valid_from" value={promotion.data.valid_from} /><DateField label="Válido até" name="valid_until" value={promotion.data.valid_until} /><LimitField label="Limite total" name="max_total_uses" value={promotion.data.max_total_uses} /><LimitField label="Usos por account" name="max_uses_per_account" value={promotion.data.max_uses_per_account} /></div>
          </FieldGroup>{update.error && <QueryError error={update.error} />}<Button type="submit" disabled={update.isPending || promotion.data.status === "ARCHIVED"}>{update.isPending ? "Salvando…" : "Salvar alterações"}</Button>
        </form></CardContent></Card>
        {kind === "vouchers" && <Card><CardHeader><CardTitle>Resgatar para um account</CardTitle><CardDescription>O crédito será lançado uma vez após a confirmação.</CardDescription></CardHeader><CardContent><form className="flex flex-col gap-4" onSubmit={(event) => { event.preventDefault(); redeem.mutate(); }}><FieldGroup className="grid gap-4 md:grid-cols-2"><Field><FieldLabel htmlFor="redemption-account">Account ID</FieldLabel><Input id="redemption-account" value={accountId} onChange={(event) => setAccountId(event.target.value.trim())} required /></Field><Field><FieldLabel htmlFor="redemption-transaction">ID da operação</FieldLabel><Input id="redemption-transaction" value={transactionId} onChange={(event) => setTransactionId(event.target.value.trim())} required maxLength={255} /><FieldDescription>Use o mesmo ID ao tentar novamente esta operação.</FieldDescription></Field></FieldGroup>{redeem.error && <QueryError error={redeem.error} />}<Button disabled={redeem.isPending}>{redeem.isPending ? "Resgatando…" : `Conceder ${promotion.data.credit_units} créditos`}</Button></form>{redemption && <p role="status" className="mt-4 text-sm">Crédito lançado. <Link className="text-primary underline" to={`/accounts/${accountId}`}>Abrir account e extrato</Link></p>}</CardContent></Card>}
        <Card><CardHeader><CardTitle>Histórico de alterações</CardTitle><CardDescription>Estado, validade e limites anteriores.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3">{history.isLoading && <QueryLoading />}{history.error && <QueryError error={history.error} />}{history.data?.items.map((item) => <article key={`${item.version}-${item.action}`} className="border-b pb-3"><p className="text-sm font-medium">Versão {item.version} · {item.action} · {formatDate(item.occurred_at)}</p><p className="break-all font-mono text-xs text-muted-foreground">{JSON.stringify(item.after_snapshot)}</p></article>)}</CardContent></Card>
      </>}
    </main>
  );
}

function DateField({ label, name, value }: { label: string; name: string; value: string | null }) {
  return <Field><FieldLabel htmlFor={name}>{label}</FieldLabel><Input id={name} name={name} type="datetime-local" defaultValue={dateTimeForInput(value)} /><FieldDescription>Vazio remove esse limite de validade.</FieldDescription></Field>;
}

function LimitField({ label, name, value }: { label: string; name: string; value: number | null }) {
  return <Field><FieldLabel htmlFor={name}>{label}</FieldLabel><Input id={name} name={name} type="number" min="1" defaultValue={value ?? ""} placeholder="Ilimitado" /><FieldDescription>Vazio significa ilimitado.</FieldDescription></Field>;
}

function Stat({ label, value }: { label: string; value: string }) {
  return <div><p className="text-xs text-muted-foreground">{label}</p><p className="text-sm font-medium">{value}</p></div>;
}

function couponBenefit(promotion: components["schemas"]["PromotionResponse"]) {
  return promotion.discount_kind === "PERCENTAGE" ? `${(promotion.discount_value ?? 0) / 100}%` : `${promotion.currency} ${((promotion.discount_value ?? 0) / 100).toFixed(2)}`;
}

function availabilityLabel(value: string) {
  const labels: Record<string, string> = {
    AVAILABLE: "Disponível",
    NOT_STARTED: "Ainda não válido",
    EXPIRED: "Expirado",
    EXHAUSTED: "Limite total atingido",
    DISABLED: "Desativado",
    ARCHIVED: "Arquivado",
  };
  return labels[value] ?? value;
}

function dateOrNull(value: string | null): string | null { return value ? new Date(value).toISOString() : null; }
function dateTimeForInput(value: string | null): string { return value ? new Date(new Date(value).getTime() - new Date(value).getTimezoneOffset() * 60_000).toISOString().slice(0, 16) : ""; }
