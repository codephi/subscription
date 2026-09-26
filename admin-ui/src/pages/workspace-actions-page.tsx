import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getBillingConfig,
  grantDirectCredit,
  reconcileCredits,
  reconcileProvisioning,
  updateBillingConfig,
} from "@/api/billing-api";
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
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { clearPendingKey, pendingKey } from "@/lib/idempotency";

export function WorkspaceActionsPage() {
  const { workspaceId } = useParams();
  if (!workspaceId) return <p>Workspace ID ausente.</p>;
  return <WorkspaceActions workspaceId={workspaceId} />;
}

function WorkspaceActions({ workspaceId }: { workspaceId: string }) {
  const client = useQueryClient();
  const config = useQuery({
    queryKey: ["billing-config", workspaceId],
    queryFn: () => getBillingConfig(workspaceId),
  });
  const [creditReview, setCreditReview] = useState<{
    transactionId: string;
    units: string;
    description: string;
  } | null>(null);
  const credit = useMutation({
    mutationFn: async (input: NonNullable<typeof creditReview>) => {
      const scope = `credit:${workspaceId}`;
      const result = await grantDirectCredit(
        workspaceId,
        pendingKey(scope, input.transactionId),
        {
          transaction_id: input.transactionId,
          credit_units: input.units,
          description: input.description || null,
          external_reference: null,
          metadata: {},
        },
      );
      clearPendingKey(scope);
      return result;
    },
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["wallets", workspaceId] });
      await client.invalidateQueries({ queryKey: ["credits", workspaceId] });
    },
  });
  const update = useMutation({
    mutationFn: (body: {
      direct_credit_enabled: boolean;
      recurring_credit_enabled: boolean;
      expected_version: number;
    }) => updateBillingConfig(workspaceId, body),
    onSuccess: async () => {
      await client.invalidateQueries({
        queryKey: ["billing-config", workspaceId],
      });
    },
  });
  const ledger = useMutation({
    mutationFn: () => reconcileCredits(workspaceId),
  });
  const provisioning = useMutation({
    mutationFn: () => reconcileProvisioning(workspaceId),
    onSuccess: async () => {
      await client.invalidateQueries({
        queryKey: ["provisioning", workspaceId],
      });
      await client.invalidateQueries({ queryKey: ["wallets", workspaceId] });
    },
  });

  function submitCredit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setCreditReview({
      transactionId: String(form.get("transaction_id") ?? "").trim(),
      units: String(form.get("credit_units") ?? "").trim(),
      description: String(form.get("description") ?? "").trim(),
    });
  }

  function submitConfig(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!config.data) return;
    const form = new FormData(event.currentTarget);
    update.mutate({
      direct_credit_enabled: form.has("direct_credit_enabled"),
      recurring_credit_enabled: form.has("recurring_credit_enabled"),
      expected_version: config.data.version,
    });
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link
          to={`/workspaces/${workspaceId}`}
          className="text-sm text-primary hover:underline"
        >
          ← Voltar ao workspace
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">
          Ações administrativas
        </h1>
        <p className="break-all font-mono text-sm text-muted-foreground">
          {workspaceId}
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>Configuração de Billing</CardTitle>
          <CardDescription>
            As alterações usam a versão atual para detectar conflitos.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {config.isLoading && <QueryLoading />}
          {config.error && <QueryError error={config.error} />}
          {config.data && (
            <form
              key={config.data.version}
              onSubmit={submitConfig}
              className="flex flex-col gap-4"
            >
              <FieldGroup>
                <Field orientation="horizontal">
                  <Checkbox
                    id="direct_credit_enabled"
                    name="direct_credit_enabled"
                    defaultChecked={config.data.direct_credit_enabled}
                  />
                  <FieldLabel htmlFor="direct_credit_enabled">
                    Permitir crédito direto
                  </FieldLabel>
                </Field>
                <Field orientation="horizontal">
                  <Checkbox
                    id="recurring_credit_enabled"
                    name="recurring_credit_enabled"
                    defaultChecked={config.data.recurring_credit_enabled}
                  />
                  <FieldLabel htmlFor="recurring_credit_enabled">
                    Permitir crédito recorrente
                  </FieldLabel>
                </Field>
              </FieldGroup>
              <p className="text-xs text-muted-foreground">
                Versão {config.data.version}
              </p>
              <Button type="submit" disabled={update.isPending}>
                Salvar configuração
              </Button>
            </form>
          )}
          {update.error && <QueryError error={update.error} />}
          {update.data && (
            <p role="status" className="mt-3 text-sm">
              Configuração salva na versão {update.data.version}.
            </p>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Crédito direto</CardTitle>
          <CardDescription>
            Revise o valor e a transação. A mesma tentativa reutiliza sua chave
            de idempotência.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submitCredit} className="flex flex-col gap-4">
            <FieldGroup className="grid gap-4 md:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="transaction_id">Transação ID</FieldLabel>
                <Input
                  id="transaction_id"
                  name="transaction_id"
                  required
                  maxLength={255}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="credit_units">
                  Unidades de crédito
                </FieldLabel>
                <Input
                  id="credit_units"
                  name="credit_units"
                  required
                  inputMode="numeric"
                  pattern="[0-9]+"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="description">Descrição</FieldLabel>
                <Input id="description" name="description" />
              </Field>
            </FieldGroup>
            <Button
              type="submit"
              disabled={credit.isPending || !config.data?.direct_credit_enabled}
            >
              Revisar crédito
            </Button>
          </form>
          {!config.data?.direct_credit_enabled && (
            <p className="mt-3 text-xs text-muted-foreground">
              Crédito direto desabilitado neste workspace.
            </p>
          )}
          {credit.error && <QueryError error={credit.error} />}
          {credit.data && (
            <p role="status" className="mt-3 break-all text-sm">
              Crédito criado: lote {credit.data.credit_lot_id}; lançamento{" "}
              {credit.data.entry.customer_wallet_entry_id}.
            </p>
          )}
          <AlertDialog
            open={!!creditReview}
            onOpenChange={(open) => {
              if (!open) setCreditReview(null);
            }}
          >
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Conceder crédito?</AlertDialogTitle>
                <AlertDialogDescription>
                  Workspace {workspaceId}: {creditReview?.units} unidades na
                  transação {creditReview?.transactionId}. Esta operação altera
                  o saldo.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Voltar</AlertDialogCancel>
                <AlertDialogAction
                  disabled={credit.isPending}
                  onClick={() => {
                    if (creditReview) credit.mutate(creditReview);
                    setCreditReview(null);
                  }}
                >
                  Confirmar crédito
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Reconciliações</CardTitle>
          <CardDescription>
            Compare a carteira com o extrato ou tente completar o
            provisionamento.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              disabled={ledger.isPending}
              onClick={() => ledger.mutate()}
            >
              Conferir créditos
            </Button>
            <Button
              variant="outline"
              disabled={provisioning.isPending}
              onClick={() => provisioning.mutate()}
            >
              Reconciliar provisionamento
            </Button>
          </div>
          {ledger.error && <QueryError error={ledger.error} />}
          {provisioning.error && <QueryError error={provisioning.error} />}
          {ledger.data && (
            <p role="status" className="text-sm">
              Créditos:{" "}
              {ledger.data.consistent ? "consistentes" : "divergentes"}. Saldo{" "}
              {ledger.data.wallet_balance_credit_units}; extrato{" "}
              {ledger.data.ledger_balance_credit_units}; lotes disponíveis{" "}
              {ledger.data.available_lot_credit_units}.
            </p>
          )}
          {provisioning.data && (
            <p role="status" className="text-sm">
              Provisionamento: {provisioning.data.status} (
              {provisioning.data.materialized_item_wallets}/
              {provisioning.data.expected_item_wallets} item wallets).
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
