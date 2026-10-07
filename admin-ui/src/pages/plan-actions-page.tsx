import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, useParams } from "react-router-dom";
import {
  cancelCustomerPlan,
  getCustomerPlan,
  revokeCustomerPlan,
  transitionCustomerPlan,
} from "@/api/plan-api";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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
import { clearPendingKey, pendingKey } from "@/lib/idempotency";

interface TransitionInput {
  newPlanId: string;
  kind: "UPGRADE" | "DOWNGRADE";
  bindingId: string;
  transactionId: string;
  actorReference: string;
}

export function PlanActionsPage() {
  const { accountId, planId } = useParams();
  if (!accountId || !planId) return <p>Plano ou account ausente.</p>;
  return <PlanActions accountId={accountId} planId={planId} />;
}

function PlanActions({
  accountId,
  planId,
}: {
  accountId: string;
  planId: string;
}) {
  const client = useQueryClient();
  const [transitionReview, setTransitionReview] =
    useState<TransitionInput | null>(null);
  const plan = useQuery({
    queryKey: ["customer-plan", accountId, planId],
    queryFn: () => getCustomerPlan(accountId, planId),
  });
  const refresh = async () => {
    await client.invalidateQueries({
      queryKey: ["customer-plan", accountId, planId],
    });
    await client.invalidateQueries({ queryKey: ["plans", accountId] });
  };
  const cancel = useMutation({
    mutationFn: () => cancelCustomerPlan(accountId, planId),
    onSuccess: refresh,
  });
  const revoke = useMutation({
    mutationFn: (body: { reason: string; actor_reference: string }) =>
      revokeCustomerPlan(accountId, planId, body),
    onSuccess: refresh,
  });
  const transition = useMutation({
    mutationFn: async (input: TransitionInput) => {
      const scope = `transition:${accountId}:${planId}`;
      const result = await transitionCustomerPlan(
        accountId,
        planId,
        pendingKey(scope, input.transactionId),
        {
          new_plan_version_id: input.newPlanId,
          transition_kind: input.kind,
          payment_method_binding_id: input.bindingId || null,
          transaction_id: input.transactionId,
          actor_reference: input.actorReference,
        },
      );
      clearPendingKey(scope);
      return result;
    },
    onSuccess: refresh,
  });

  function submitTransition(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setTransitionReview({
      newPlanId: String(form.get("new_plan_version_id") ?? "").trim(),
      kind: String(form.get("transition_kind")) as TransitionInput["kind"],
      bindingId: String(form.get("payment_method_binding_id") ?? "").trim(),
      transactionId: String(form.get("transaction_id") ?? "").trim(),
      actorReference: String(form.get("actor_reference") ?? "").trim(),
    });
  }

  function submitRevoke(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    revoke.mutate({
      reason: String(form.get("reason") ?? "").trim(),
      actor_reference: String(form.get("actor_reference") ?? "").trim(),
    });
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link
          className="text-sm text-primary hover:underline"
          to={`/accounts/${accountId}`}
        >
          ← Voltar ao account
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">
          Ações do plano
        </h1>
        <p className="break-all font-mono text-sm text-muted-foreground">
          {planId}
        </p>
      </header>
      {plan.isLoading && <QueryLoading />}
      {plan.error && <QueryError error={plan.error} />}
      {plan.data && (
        <Card>
          <CardHeader>
            <CardTitle>
              Estado atual{" "}
              <Badge variant="outline">{plan.data.commercial_status}</Badge>
            </CardTitle>
            <CardDescription>
              Versão {plan.data.plan_version_id}; renovação{" "}
              {plan.data.renewal_status}; ciclo{" "}
              {plan.data.current_cycle?.cycle_ordinal ?? "—"}.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <AlertDialog>
              <AlertDialogTrigger
                render={
                  <Button
                    variant="outline"
                    disabled={
                      cancel.isPending || plan.data.cancel_at_period_end
                    }
                  />
                }
              >
                Cancelar ao fim do período
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Agendar cancelamento?</AlertDialogTitle>
                  <AlertDialogDescription>
                    O plano {planId} será marcado para cancelamento ao fim do
                    período atual.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Voltar</AlertDialogCancel>
                  <AlertDialogAction onClick={() => cancel.mutate()}>
                    Confirmar
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
            {cancel.error && <QueryError error={cancel.error} />}
            {cancel.data && (
              <p role="status" className="mt-3 text-sm">
                Cancelamento ao fim do período:{" "}
                {cancel.data.cancel_at_period_end ? "agendado" : "não agendado"}
                .
              </p>
            )}
          </CardContent>
        </Card>
      )}
      <Card>
        <CardHeader>
          <CardTitle>Transição de plano</CardTitle>
          <CardDescription>
            Uma transição paga requer vínculo de método de pagamento. A chave
            permanece estável nas tentativas da mesma transação.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submitTransition} className="flex flex-col gap-4">
            <FieldGroup className="grid gap-4 md:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="new_plan_version_id">
                  Nova versão do plano ID
                </FieldLabel>
                <Input
                  id="new_plan_version_id"
                  name="new_plan_version_id"
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="transition_kind">Tipo</FieldLabel>
                <Select name="transition_kind" defaultValue="DOWNGRADE">
                  <SelectTrigger id="transition_kind" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="DOWNGRADE">DOWNGRADE</SelectItem>
                      <SelectItem value="UPGRADE">UPGRADE</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              <Field>
                <FieldLabel htmlFor="payment_method_binding_id">
                  Vínculo de pagamento ID
                </FieldLabel>
                <Input
                  id="payment_method_binding_id"
                  name="payment_method_binding_id"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="transaction_id">Transação ID</FieldLabel>
                <Input id="transaction_id" name="transaction_id" required />
              </Field>
              <Field>
                <FieldLabel htmlFor="actor_reference">
                  Referência operacional
                </FieldLabel>
                <Input id="actor_reference" name="actor_reference" required />
              </Field>
            </FieldGroup>
            <Button type="submit" disabled={transition.isPending}>
              Revisar transição
            </Button>
          </form>
          {transition.error && <QueryError error={transition.error} />}
          {transition.data && (
            <p role="status" className="mt-3 text-sm">
              Resultado: {transition.data.result}. Confira o estado atualizado
              acima.
            </p>
          )}
          <AlertDialog
            open={!!transitionReview}
            onOpenChange={(open) => {
              if (!open) setTransitionReview(null);
            }}
          >
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Executar transição?</AlertDialogTitle>
                <AlertDialogDescription>
                  {transitionReview?.kind} para {transitionReview?.newPlanId};
                  transação {transitionReview?.transactionId}. O resultado pode
                  exigir pagamento.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Voltar</AlertDialogCancel>
                <AlertDialogAction
                  disabled={transition.isPending}
                  onClick={() => {
                    if (transitionReview) transition.mutate(transitionReview);
                    setTransitionReview(null);
                  }}
                >
                  Confirmar transição
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Revogar plano do cliente</CardTitle>
          <CardDescription>
            Use um motivo e uma referência operacional. A referência é texto
            fornecido pelo operador.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submitRevoke} className="flex flex-col gap-4">
            <FieldGroup className="grid gap-4 md:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="revoke_reason">Motivo</FieldLabel>
                <Input id="revoke_reason" name="reason" required />
              </Field>
              <Field>
                <FieldLabel htmlFor="revoke_actor">
                  Referência operacional
                </FieldLabel>
                <Input id="revoke_actor" name="actor_reference" required />
              </Field>
            </FieldGroup>
            <Button
              type="submit"
              variant="destructive"
              disabled={revoke.isPending}
            >
              Revogar
            </Button>
          </form>
          {revoke.error && <QueryError error={revoke.error} />}
          {revoke.data && (
            <p role="status" className="mt-3 text-sm">
              Estado: {revoke.data.commercial_status}.
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
