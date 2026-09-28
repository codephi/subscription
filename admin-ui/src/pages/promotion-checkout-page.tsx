import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import {
  createCheckout,
  createCheckoutCustomerPlan,
  getCheckout,
  getCheckoutPaymentMethods,
  getCheckoutPlan,
  listCheckoutCustomerPlans,
  listCheckoutOffers,
  listCheckoutPlans,
  listCheckoutWorkspaces,
  quoteCheckout,
} from "@/api/checkout-api";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { pendingKey } from "@/lib/idempotency";

type CheckoutMode = "INITIAL" | "ON_DEMAND";

export function PromotionCheckoutPage() {
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<CheckoutMode>("INITIAL");
  const [workspaceId, setWorkspaceId] = useState("");
  const [planVersionId, setPlanVersionId] = useState("");
  const [customerPlanId, setCustomerPlanId] = useState("");
  const [offerId, setOfferId] = useState("");
  const [bindingId, setBindingId] = useState("");
  const [couponCode, setCouponCode] = useState("");
  const [transactionId, setTransactionId] = useState(() => localStorage.getItem("subscription-admin:checkout-transaction") ?? `admin-${crypto.randomUUID()}`);
  const [checkoutId, setCheckoutId] = useState("");

  const workspaces = useQuery({ queryKey: ["checkout-workspaces"], queryFn: listCheckoutWorkspaces });
  const plans = useQuery({ queryKey: ["checkout-plans"], queryFn: listCheckoutPlans });
  const customerPlans = useQuery({
    queryKey: ["checkout-customer-plans", workspaceId],
    queryFn: () => listCheckoutCustomerPlans(workspaceId),
    enabled: mode === "ON_DEMAND" && !!workspaceId,
  });
  const chosenCustomerPlan = customerPlans.data?.items.find((item) => item.customer_plan_id === customerPlanId);
  const selectedPlanVersionId = mode === "INITIAL" ? planVersionId : chosenCustomerPlan?.plan_version_id ?? "";
  const plan = useQuery({
    queryKey: ["checkout-plan", selectedPlanVersionId],
    queryFn: () => getCheckoutPlan(selectedPlanVersionId),
    enabled: !!selectedPlanVersionId,
  });
  const offers = useQuery({
    queryKey: ["checkout-offers", plan.data?.subscription_id],
    queryFn: () => listCheckoutOffers(plan.data?.subscription_id),
    enabled: mode === "ON_DEMAND" && !!plan.data?.subscription_id,
  });
  const bindings = useQuery({
    queryKey: ["checkout-payment-methods", workspaceId],
    queryFn: () => getCheckoutPaymentMethods(workspaceId),
    enabled: !!workspaceId,
  });
  useEffect(() => {
    localStorage.setItem("subscription-admin:checkout-transaction", transactionId);
  }, [transactionId]);

  const savePlan = useMutation({
    mutationFn: () => createCheckoutCustomerPlan(workspaceId, pendingKey(`checkout-plan:${workspaceId}`, transactionId), planVersionId, transactionId),
    onSuccess: (result) => {
      localStorage.setItem(planStorageKey(workspaceId, planVersionId, transactionId), result.customer_plan_id);
      setCustomerPlanId(result.customer_plan_id);
    },
  });
  const quote = useMutation({
    mutationFn: () => quoteCheckout(workspaceId, {
      customer_plan_id: customerPlanId,
      checkout_kind: mode,
      on_demand_plan_id: mode === "ON_DEMAND" ? offerId : null,
      coupon_code: couponCode.trim().toUpperCase(),
    }),
  });
  const startCheckout = useMutation({
    mutationFn: () => createCheckout(workspaceId, pendingKey(`checkout:${workspaceId}`, transactionId), {
      customer_plan_id: customerPlanId,
      checkout_kind: mode,
      on_demand_plan_id: mode === "ON_DEMAND" ? offerId : null,
      transaction_id: transactionId,
      coupon_code: couponCode.trim().toUpperCase(),
      payment_method_binding_id: quote.data?.payment_required ? bindingId || null : null,
    }),
    onSuccess: (result) => {
      localStorage.setItem(checkoutStorageKey(workspaceId, transactionId), result.checkout_id);
      setCheckoutId(result.checkout_id);
    },
  });
  const checkout = useQuery({
    queryKey: ["checkout", workspaceId, checkoutId],
    queryFn: () => getCheckout(workspaceId, checkoutId),
    enabled: !!workspaceId && !!checkoutId,
    refetchInterval: (query) => query.state.data?.status === "PENDING" ? 2_000 : false,
  });
  useEffect(() => {
    if (checkout.data?.status !== "PAID" && checkout.data?.status !== "COMPLETED") return;
    void queryClient.invalidateQueries({ queryKey: ["wallets", workspaceId] });
    void queryClient.invalidateQueries({ queryKey: ["credits", workspaceId] });
    void queryClient.invalidateQueries({ queryKey: ["checkout-customer-plans", workspaceId] });
  }, [checkout.data?.status, queryClient, workspaceId]);

  function switchMode(next: CheckoutMode) {
    setMode(next);
    setCustomerPlanId("");
    setOfferId("");
    setPlanVersionId("");
    quote.reset();
  }

  function selectWorkspace(nextWorkspaceId: string) {
    setWorkspaceId(nextWorkspaceId);
    setCustomerPlanId(localStorage.getItem(planStorageKey(nextWorkspaceId, planVersionId, transactionId)) ?? "");
    setCheckoutId(localStorage.getItem(checkoutStorageKey(nextWorkspaceId, transactionId)) ?? "");
    quote.reset();
  }

  function selectPlan(nextPlanVersionId: string) {
    setPlanVersionId(nextPlanVersionId);
    setCustomerPlanId(localStorage.getItem(planStorageKey(workspaceId, nextPlanVersionId, transactionId)) ?? "");
    quote.reset();
  }

  function selectTransaction(nextTransactionId: string) {
    setTransactionId(nextTransactionId);
    setCustomerPlanId(localStorage.getItem(planStorageKey(workspaceId, planVersionId, nextTransactionId)) ?? "");
    setCheckoutId(localStorage.getItem(checkoutStorageKey(workspaceId, nextTransactionId)) ?? "");
    quote.reset();
  }

  const workspaceItems = workspaces.data?.items ?? [];
  const planItems = plans.data?.items.filter((item) => item.status === "ACTIVE") ?? [];
  const offerItems = offers.data?.items.filter((item) => item.status === "ACTIVE") ?? [];
  const bindingItems = bindings.data ?? [];
  const canQuote = !!workspaceId && !!customerPlanId && !!couponCode.trim()
    && (mode === "INITIAL" || !!offerId);
  const canStart = !!quote.data && (!quote.data.payment_required || !!bindingId);

  return (
    <main className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link className="text-sm text-primary hover:underline" to="/promotions">← Promoções</Link>
        <h1 className="text-3xl font-semibold tracking-tight">Checkout com cupom</h1>
        <p className="text-muted-foreground">A cotação não reserva usos. A confirmação do checkout revalida e reserva o cupom.</p>
      </header>
      <Card>
        <CardHeader><CardTitle>Compra</CardTitle><CardDescription>Contratações criam e guardam o CustomerPlan antes de iniciar a cobrança.</CardDescription></CardHeader>
        <CardContent>
          <FieldGroup className="grid gap-4 md:grid-cols-2">
            <Field><FieldLabel htmlFor="checkout-mode">Tipo</FieldLabel><select id="checkout-mode" className="h-9 rounded-md border bg-background px-3 text-sm" value={mode} onChange={(event) => switchMode(event.target.value as CheckoutMode)}><option value="INITIAL">Contratação inicial</option><option value="ON_DEMAND">Compra de créditos</option></select></Field>
            <Field><FieldLabel htmlFor="checkout-workspace">Workspace</FieldLabel><select id="checkout-workspace" className="h-9 rounded-md border bg-background px-3 text-sm" value={workspaceId} onChange={(event) => selectWorkspace(event.target.value)}><option value="">Selecione um workspace</option>{workspaceItems.map((item) => <option key={item.workspace_id} value={item.workspace_id}>{item.workspace_id}</option>)}</select></Field>
            {mode === "INITIAL" ? <>
              <Field><FieldLabel htmlFor="checkout-plan">Plano publicado</FieldLabel><select id="checkout-plan" className="h-9 rounded-md border bg-background px-3 text-sm" value={planVersionId} onChange={(event) => selectPlan(event.target.value)}><option value="">Selecione um plano</option>{planItems.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></Field>
              <Field><FieldLabel htmlFor="checkout-customer-plan">CustomerPlan</FieldLabel><Input id="checkout-customer-plan" value={customerPlanId} readOnly placeholder="Será criado e salvo antes do checkout" /><FieldDescription>{customerPlanId ? "Referência salva para retomar esta operação." : "Crie a adesão antes de cotar."}</FieldDescription></Field>
              <div className="md:col-span-2"><Button disabled={!workspaceId || !planVersionId || !transactionId.trim() || savePlan.isPending} onClick={() => savePlan.mutate()}>{savePlan.isPending ? "Salvando adesão…" : customerPlanId ? "Recuperar CustomerPlan" : "Criar e guardar CustomerPlan"}</Button></div>
            </> : <>
              <Field><FieldLabel htmlFor="checkout-customer-plan-existing">CustomerPlan ativo</FieldLabel><select id="checkout-customer-plan-existing" className="h-9 rounded-md border bg-background px-3 text-sm" value={customerPlanId} onChange={(event) => { setCustomerPlanId(event.target.value); setOfferId(""); quote.reset(); }}><option value="">Selecione uma adesão</option>{customerPlans.data?.items.map((item) => <option key={item.customer_plan_id} value={item.customer_plan_id}>{item.customer_plan_id} · {item.commercial_status}/{item.activation_status}</option>)}</select></Field>
              <Field><FieldLabel htmlFor="checkout-offer">Pacote de créditos</FieldLabel><select id="checkout-offer" className="h-9 rounded-md border bg-background px-3 text-sm" value={offerId} onChange={(event) => { setOfferId(event.target.value); quote.reset(); }}><option value="">Selecione uma oferta</option>{offerItems.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></Field>
            </>}
            <Field><FieldLabel htmlFor="checkout-coupon">Cupom</FieldLabel><Input id="checkout-coupon" value={couponCode} onChange={(event) => { setCouponCode(event.target.value.toUpperCase()); quote.reset(); }} required maxLength={64} /></Field>
            <Field><FieldLabel htmlFor="checkout-transaction">ID da operação</FieldLabel><Input id="checkout-transaction" value={transactionId} onChange={(event) => selectTransaction(event.target.value.trim())} required maxLength={255} /><FieldDescription>O mesmo ID recupera a adesão e o checkout em retries.</FieldDescription></Field>
            <Field><FieldLabel htmlFor="checkout-card">Cartão vinculado</FieldLabel><select id="checkout-card" className="h-9 rounded-md border bg-background px-3 text-sm" value={bindingId} onChange={(event) => setBindingId(event.target.value)}><option value="">Selecionar após cotação</option>{bindingItems.filter((item) => item.status === "ACTIVE").map((item) => <option key={item.payment_method_binding_id} value={item.payment_method_binding_id}>{item.payment_method} · {item.payment_method_binding_id}</option>)}</select><FieldDescription>Obrigatório somente se o total cotado for maior que zero.</FieldDescription></Field>
          </FieldGroup>
          {(workspaces.error || plans.error || customerPlans.error || offers.error || bindings.error || savePlan.error) && <QueryError error={workspaces.error ?? plans.error ?? customerPlans.error ?? offers.error ?? bindings.error ?? savePlan.error} />}
          <div className="mt-5 flex flex-wrap gap-2"><Button disabled={!canQuote || quote.isPending} onClick={() => quote.mutate()}>{quote.isPending ? "Cotando…" : "Calcular cotação"}</Button>{quote.error && <QueryError error={quote.error} />}</div>
        </CardContent>
      </Card>
      {quote.data && <Card><CardHeader><CardTitle>Revisão da cotação</CardTitle><CardDescription>O benefício contratado e a quantidade de créditos permanecem iguais.</CardDescription></CardHeader><CardContent className="flex flex-col gap-4">
        <dl className="grid gap-3 sm:grid-cols-3"><Amount label="Preço original" amount={quote.data.base_amount_minor} currency={quote.data.currency} /><Amount label="Desconto" amount={quote.data.discount_amount_minor} currency={quote.data.currency} /><Amount label="Total" amount={quote.data.amount_minor} currency={quote.data.currency} /></dl>
        <p className="text-sm text-muted-foreground">Benefício: {quote.data.granted_credit_units} créditos. {quote.data.payment_required ? "Será cobrado no cartão selecionado." : "Total zero: conclusão direta sem provedor."}</p>
        <Button disabled={!canStart || startCheckout.isPending || checkout.data?.status === "PENDING" || !!checkout.data?.collection_request_id} onClick={() => startCheckout.mutate()}>{startCheckout.isPending ? "Iniciando…" : checkout.data?.status === "PENDING" ? "Checkout em andamento…" : "Confirmar checkout"}</Button>
        {startCheckout.error && <QueryError error={startCheckout.error} />}
      </CardContent></Card>}
      {checkout.data && <Card><CardHeader><CardTitle>Resultado do checkout</CardTitle><CardDescription>Checkout {checkout.data.checkout_id}</CardDescription></CardHeader><CardContent><p role="status">Estado: {checkout.data.status} · {checkout.data.payment_required ? "pagamento pendente" : "sem pagamento pendente"}</p>{checkout.data.collection_request_id && <p className="mt-2 text-sm">Cobrança: <Link className="text-primary underline" to={`/billing/collections/${checkout.data.collection_request_id}`}>{checkout.data.collection_request_id}</Link></p>}</CardContent></Card>}
      {checkout.isLoading && <QueryLoading />}{checkout.error && <QueryError error={checkout.error} />}
    </main>
  );
}

function Amount({ label, amount, currency }: { label: string; amount: number; currency: string }) {
  return <div><dt className="text-xs text-muted-foreground">{label}</dt><dd className="text-lg font-semibold">{new Intl.NumberFormat("pt-BR", { style: "currency", currency }).format(amount / 100)}</dd></div>;
}

function planStorageKey(workspaceId: string, planVersionId: string, transactionId: string): string {
  return `subscription-admin:checkout-plan:${workspaceId}:${planVersionId}:${transactionId}`;
}

function checkoutStorageKey(workspaceId: string, transactionId: string): string {
  return `subscription-admin:checkout:${workspaceId}:${transactionId}`;
}
