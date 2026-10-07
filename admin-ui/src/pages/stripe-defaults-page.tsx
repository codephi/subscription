import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { KeyRound } from "lucide-react";
import {
  getDefaultStripeCredentials,
  updateDefaultStripeCredentials,
} from "@/api/billing-api";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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
import { Badge } from "@/components/ui/badge";

export function StripeDefaultsPage() {
  const credentials = useQuery({
    queryKey: ["stripe-default-credentials"],
    queryFn: getDefaultStripeCredentials,
  });
  return (
    <main className="flex flex-col gap-6">
      <PageHeading />
      {credentials.isLoading && <QueryLoading />}
      {credentials.error && <QueryError error={credentials.error} />}
      {credentials.data && <CredentialSettings credentials={credentials.data} />}
    </main>
  );
}

function PageHeading() {
  return (
    <header className="flex flex-col gap-2">
      <h1 className="text-3xl font-semibold tracking-tight">Credenciais padrão</h1>
      <p className="text-sm text-muted-foreground">
        Novos accounts recebem uma conexão Stripe própria usando esta conta.
      </p>
    </header>
  );
}

function CredentialSettings({
  credentials,
}: {
  credentials: Awaited<ReturnType<typeof getDefaultStripeCredentials>>;
}) {
  const client = useQueryClient();
  const [saved, setSaved] = useState(false);
  const update = useMutation({
    mutationFn: updateDefaultStripeCredentials,
    onSuccess: async () => {
      setSaved(true);
      await client.invalidateQueries({ queryKey: ["stripe-default-credentials"] });
    },
  });
  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <KeyRound aria-hidden="true" />
          <CardTitle>Stripe para novos accounts</CardTitle>
        </div>
        <CardDescription>
          As chaves são armazenadas cifradas. Accounts existentes mantêm a própria configuração.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        {credentials.configured && <CredentialStatus credentials={credentials} />}
        <CredentialForm credentials={credentials} saving={update.isPending} onSubmit={submit} />
        {saved && <p role="status" className="text-sm text-muted-foreground">Credenciais padrão salvas.</p>}
        {update.error && <QueryError error={update.error} />}
      </CardContent>
    </Card>
  );

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaved(false);
    const form = new FormData(event.currentTarget);
    update.mutate({
      expected_version: credentials.configuration_version,
      secret_key: valueOrUndefined(form, "secret_key"),
      webhook_secret: valueOrUndefined(form, "webhook_secret"),
    });
  }
}

function CredentialStatus({
  credentials,
}: {
  credentials: Awaited<ReturnType<typeof getDefaultStripeCredentials>>;
}) {
  return (
    <Alert>
      <AlertTitle className="flex items-center gap-2">
        Configuração ativa <Badge variant="secondary">{credentials.environment}</Badge>
      </AlertTitle>
      <AlertDescription>
        Conta {credentials.account_reference}. API configurada; webhook {credentials.webhook_secret_configured ? "configurado" : "pendente"}.
      </AlertDescription>
    </Alert>
  );
}

function CredentialForm({
  credentials,
  saving,
  onSubmit,
}: {
  credentials: Awaited<ReturnType<typeof getDefaultStripeCredentials>>;
  saving: boolean;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-4">
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="default-stripe-secret">Chave secreta</FieldLabel>
          <Input id="default-stripe-secret" name="secret_key" type="password" autoComplete="new-password" placeholder={credentials.api_secret_configured ? "Deixe em branco para manter" : "sk_test_…"} required={!credentials.configured} />
          <FieldDescription>Use uma chave sk_test_ ou sk_live_. A tela nunca mostra o valor salvo.</FieldDescription>
        </Field>
        <Field>
          <FieldLabel htmlFor="default-stripe-webhook">Segredo do webhook</FieldLabel>
          <Input id="default-stripe-webhook" name="webhook_secret" type="password" autoComplete="new-password" placeholder={credentials.webhook_secret_configured ? "Deixe em branco para manter" : "whsec_…"} required={!credentials.webhook_secret_configured} />
          <FieldDescription>O segredo assina os eventos recebidos pela Subscription.</FieldDescription>
        </Field>
      </FieldGroup>
      <div>
        <Button type="submit" disabled={saving}>
          {saving ? "Salvando…" : credentials.configured ? "Salvar alterações" : "Salvar credenciais padrão"}
        </Button>
      </div>
    </form>
  );
}

function valueOrUndefined(form: FormData, name: string) {
  const value = String(form.get(name) ?? "").trim();
  return value.length === 0 ? undefined : value;
}
