import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Check, Copy, RefreshCw } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, useParams } from "react-router-dom";
import {
  createStripeIntegration,
  listIntegrationProviders,
  listWorkspaceIntegrations,
  testStripeIntegration,
  updateStripeIntegration,
} from "@/api/billing-api";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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

type WorkspaceIntegration = Awaited<ReturnType<typeof listWorkspaceIntegrations>>[number];

export function WorkspaceIntegrationsPage() {
  const { workspaceId } = useParams();
  if (!workspaceId) return <p>Workspace ID ausente.</p>;
  return <WorkspaceIntegrations workspaceId={workspaceId} />;
}

function WorkspaceIntegrations({ workspaceId }: { workspaceId: string }) {
  const client = useQueryClient();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [environment, setEnvironment] = useState("TEST");
  const [formError, setFormError] = useState<Error | null>(null);
  const [savingCredentials, setSavingCredentials] = useState(false);
  const [savingSecrets, setSavingSecrets] = useState(false);
  const providers = useQuery({
    queryKey: ["integration-providers"],
    queryFn: listIntegrationProviders,
  });
  const integrations = useQuery({
    queryKey: ["workspace-integrations", workspaceId],
    queryFn: () => listWorkspaceIntegrations(workspaceId),
  });
  const selected = integrations.data?.find(
    (integration) => integration.billing_connection_id === selectedId,
  );
  const test = useMutation({
    mutationFn: () =>
      selected
        ? testStripeIntegration(workspaceId, selected.billing_connection_id)
        : Promise.reject(new Error("Selecione uma integração.")),
  });

  async function submitCredentials(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const secretKey = String(form.get("stripe_secret_key") ?? "").trim();
    if (!secretKey) return;
    setFormError(null);
    try {
      setSavingCredentials(true);
      const created = await createStripeIntegration(workspaceId, {
        secret_key: secretKey,
        environment,
        existing_customer_reference:
          String(form.get("existing_customer_reference") ?? "").trim() || null,
      });
      formElement.reset();
      setSelectedId(created.billing_connection_id);
      await client.invalidateQueries({
        queryKey: ["workspace-integrations", workspaceId],
      });
    } catch (error) {
      setFormError(asError(error));
    } finally {
      setSavingCredentials(false);
    }
  }

  async function submitSecrets(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected) return;
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const secretKey = String(form.get("replacement_secret_key") ?? "").trim();
    const webhookSecret = String(form.get("stripe_webhook_secret") ?? "").trim();
    if (!secretKey && !webhookSecret) {
      setFormError(new Error("Informe a chave do webhook ou uma nova chave API."));
      return;
    }
    setFormError(null);
    try {
      setSavingSecrets(true);
      await updateStripeIntegration(workspaceId, selected.billing_connection_id, {
        expected_version: selected.configuration_version,
        ...(secretKey ? { secret_key: secretKey } : {}),
        ...(webhookSecret ? { webhook_secret: webhookSecret } : {}),
      });
      formElement.reset();
      await client.invalidateQueries({
        queryKey: ["workspace-integrations", workspaceId],
      });
    } catch (error) {
      setFormError(asError(error));
    } finally {
      setSavingSecrets(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link
          to={`/workspaces/${workspaceId}`}
          className="inline-flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft aria-hidden="true" />
          Voltar ao workspace
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">Integrações</h1>
        <p className="break-all font-mono text-sm text-muted-foreground">
          {workspaceId}
        </p>
      </header>
      {providers.isLoading && <QueryLoading />}
      {providers.error && <QueryError error={providers.error} />}
      {integrations.isLoading && <QueryLoading />}
      {integrations.error && <QueryError error={integrations.error} />}
      {formError && <QueryError error={formError} />}
      {test.error && <QueryError error={test.error} />}
      {test.data && (
        <Alert>
          <Check aria-hidden="true" />
          <AlertTitle>Conexão validada</AlertTitle>
          <AlertDescription>
            Conta {test.data.account_reference} · ambiente {test.data.environment}.
          </AlertDescription>
        </Alert>
      )}
      {integrations.data && integrations.data.length > 0 && (
        <section className="flex flex-col gap-3" aria-label="Integrações cadastradas">
          {integrations.data.map((integration) => (
            <Card key={integration.billing_connection_id}>
              <CardHeader className="flex flex-row items-start justify-between gap-4">
                <div className="flex flex-col gap-1">
                  <CardTitle>{integration.provider}</CardTitle>
                  <CardDescription>
                    {integration.account_reference} · {environmentLabel(integration.environment)}
                  </CardDescription>
                </div>
                <Badge variant={integration.status === "ACTIVE" ? "secondary" : "outline"}>
                  {statusLabel(integration.status)}
                </Badge>
              </CardHeader>
              <CardContent className="flex flex-wrap items-center gap-2">
                <span className="text-sm text-muted-foreground">
                  Chave API: {integration.api_secret_configured ? "Configurada" : "Pendente"}
                  {" · "}Webhook: {integration.webhook_secret_configured ? "Configurado" : "Pendente"}
                </span>
                {integration.environment !== "LEGACY" && (
                  <Button
                    variant="outline"
                    disabled={test.isPending}
                    onClick={() => test.mutate()}
                  >
                    <RefreshCw data-icon="inline-start" />
                    Testar conexão
                  </Button>
                )}
                <Button
                  variant="outline"
                  onClick={() => setSelectedId(integration.billing_connection_id)}
                >
                  {integration.status === "ACTIVE" ? "Gerenciar" : "Continuar configuração"}
                </Button>
              </CardContent>
            </Card>
          ))}
        </section>
      )}
      {selected && selected.environment !== "LEGACY" ? (
        <SelectedIntegrationForm
          integration={selected}
          pending={savingSecrets}
          onSubmit={submitSecrets}
        />
      ) : (
        <CredentialsForm
          providers={providers.data ?? []}
          environment={environment}
          onEnvironmentChange={setEnvironment}
          pending={savingCredentials}
          onSubmit={submitCredentials}
        />
      )}
      {selected && selected.status === "ACTIVE" && (
        <Alert>
          <Check aria-hidden="true" />
          <AlertTitle>Integração pronta</AlertTitle>
          <AlertDescription>
            As chaves ficam guardadas na API. A interface não pode recuperá-las.
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}

function CredentialsForm({
  providers,
  environment,
  onEnvironmentChange,
  pending,
  onSubmit,
}: {
  providers: Awaited<ReturnType<typeof listIntegrationProviders>>;
  environment: string;
  onEnvironmentChange: (environment: string) => void;
  pending: boolean;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  const stripe = providers.find((provider) => provider.provider === "STRIPE");
  return (
    <Card>
      <CardHeader>
        <CardTitle>Conectar Stripe</CardTitle>
        <CardDescription>
          Informe uma chave secreta da própria conta. Ela será validada e guardada pela API.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {!stripe && <p>Nenhum provedor de Billing está disponível.</p>}
        {stripe && (
          <form onSubmit={onSubmit} className="flex flex-col gap-4">
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="stripe_secret_key">Chave secreta do Stripe</FieldLabel>
                <Input
                  id="stripe_secret_key"
                  name="stripe_secret_key"
                  type="password"
                  autoComplete="new-password"
                  required
                  pattern="sk_(test|live)_.+"
                  placeholder="sk_test_…"
                  aria-describedby="stripe-key-help"
                />
                <p id="stripe-key-help" className="text-xs text-muted-foreground">
                  Copie em Stripe → Developers → API keys. Chaves publicáveis pk_ não funcionam.
                </p>
              </Field>
              <Field>
                <FieldLabel htmlFor="stripe_environment">Ambiente</FieldLabel>
                <Select value={environment} onValueChange={(value) => value && onEnvironmentChange(value)}>
                  <SelectTrigger id="stripe_environment" aria-label="Ambiente Stripe">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="TEST">Teste</SelectItem>
                      <SelectItem value="LIVE">Produção</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              <details className="rounded-md border p-3">
                <summary className="cursor-pointer text-sm font-medium">
                  Opções avançadas
                </summary>
                <Field className="mt-3">
                  <FieldLabel htmlFor="existing_customer_reference">
                    Cliente Stripe existente (opcional)
                  </FieldLabel>
                  <Input
                    id="existing_customer_reference"
                    name="existing_customer_reference"
                    autoComplete="off"
                    placeholder="cus_…"
                  />
                  <p className="text-xs text-muted-foreground">
                    Deve pertencer a esta conta e ambiente. Se vazio, o cliente será criado ao salvar o primeiro cartão.
                  </p>
                </Field>
              </details>
            </FieldGroup>
            <Button type="submit" disabled={pending}>
              {pending ? "Validando…" : "Validar e continuar"}
            </Button>
          </form>
        )}
      </CardContent>
    </Card>
  );
}

function SelectedIntegrationForm({
  integration,
  pending,
  onSubmit,
}: {
  integration: WorkspaceIntegration;
  pending: boolean;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          {integration.status === "ACTIVE" ? "Credenciais e webhook" : "Etapa 2 · Configurar webhook"}
        </CardTitle>
        <CardDescription>
          A chave de assinatura do webhook é necessária para confirmar pagamentos.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <Alert>
          <AlertTitle>Cadastre o endpoint no Stripe</AlertTitle>
          <AlertDescription>
            <ol className="list-decimal pl-5">
              <li>Abra Developers → Webhooks na mesma conta e ambiente.</li>
              <li>Adicione um endpoint para os cinco eventos abaixo.</li>
              <li>Copie o segredo de assinatura e informe-o neste formulário.</li>
            </ol>
            <p className="mt-3 break-all font-mono text-xs">
              {integration.webhook_url ?? "Configure PUBLIC_API_BASE_URL na API para exibir a URL completa."}
            </p>
          </AlertDescription>
          {integration.webhook_url && (
            <div className="mt-2">
              <CopyButton value={integration.webhook_url} />
            </div>
          )}
        </Alert>
        <div>
          <p className="mb-2 text-sm font-medium">Eventos necessários</p>
          <ul className="grid gap-1 font-mono text-xs text-muted-foreground sm:grid-cols-2">
            <li>payment_intent.succeeded</li>
            <li>payment_intent.payment_failed</li>
            <li>payment_intent.canceled</li>
            <li>payment_intent.requires_action</li>
            <li>charge.refunded</li>
          </ul>
        </div>
        <form onSubmit={onSubmit} className="flex flex-col gap-4">
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="stripe_webhook_secret">
                Segredo de assinatura (whsec_…)
              </FieldLabel>
              <Input
                id="stripe_webhook_secret"
                name="stripe_webhook_secret"
                type="password"
                autoComplete="new-password"
                pattern="whsec_.+"
                required={integration.status !== "ACTIVE"}
                placeholder={integration.webhook_secret_configured ? "Configurado; informe apenas para substituir" : "whsec_…"}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="replacement_secret_key">
                Substituir chave API (opcional)
              </FieldLabel>
              <Input
                id="replacement_secret_key"
                name="replacement_secret_key"
                type="password"
                autoComplete="new-password"
                pattern="sk_(test|live)_.+"
                placeholder="Deixe vazio para manter a chave atual"
              />
            </Field>
          </FieldGroup>
          <div className="flex flex-wrap gap-2">
            <Button type="submit" disabled={pending}>
              {pending ? "Salvando…" : "Salvar e concluir"}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => window.location.reload()}
            >
              Atualizar estado
            </Button>
          </div>
        </form>
        <p className="text-xs text-muted-foreground">
          Configurar o segredo não confirma entrega. Teste o endpoint no Stripe e use “Testar conexão” para verificar a chave API.
        </p>
      </CardContent>
    </Card>
  );
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={async () => {
        await navigator.clipboard.writeText(value);
        setCopied(true);
      }}
    >
      {copied ? <Check data-icon="inline-start" /> : <Copy data-icon="inline-start" />}
      {copied ? "Copiada" : "Copiar URL"}
    </Button>
  );
}

function statusLabel(status: string): string {
  return status === "ACTIVE" ? "Ativa" : status === "PENDING_SETUP" ? "Configuração pendente" : status;
}

function environmentLabel(environment: string): string {
  return environment === "TEST" ? "Teste" : environment === "LIVE" ? "Produção" : "Legada";
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error("A API não conseguiu salvar a integração.");
}
