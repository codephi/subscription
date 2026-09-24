import { useQuery } from "@tanstack/react-query";
import {
  ArrowRight,
  Clock3,
  Webhook,
  WalletCards,
  ListTodo,
  Send,
  TriangleAlert,
} from "lucide-react";
import { Link } from "react-router-dom";
import { getOperations } from "@/api/client";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { QueryError, QueryLoading } from "@/components/query-feedback";

const metrics = [
  {
    key: "pending_collections",
    label: "Cobranças pendentes",
    icon: Clock3,
    detail: "Solicitações aguardando conclusão",
  },
  {
    key: "webhook_failures",
    label: "Falhas de webhook",
    icon: Webhook,
    detail: "Eventos recebidos com falha",
  },
  {
    key: "unprocessed_webhooks",
    label: "Webhooks não processados",
    icon: ListTodo,
    detail: "Ainda sem resultado final",
  },
  {
    key: "open_unmatched_payments",
    label: "Pagamentos não conciliados",
    icon: WalletCards,
    detail: "Casos abertos para análise",
  },
  {
    key: "outbox_backlog",
    label: "Outbox pendente",
    icon: Send,
    detail: "Eventos aguardando entrega",
  },
  {
    key: "outbox_dead_letters",
    label: "Dead letters",
    icon: TriangleAlert,
    detail: "Eventos que exigem investigação",
  },
] as const;

export function OverviewPage() {
  const operations = useQuery({
    queryKey: ["operations"],
    queryFn: getOperations,
    refetchInterval: 30_000,
  });
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <p className="text-sm font-medium text-muted-foreground">
          OPERAÇÃO · SUBSCRIPTION
        </p>
        <h1 className="text-3xl font-semibold tracking-tight">Visão geral</h1>
        <p className="text-muted-foreground">
          Indicadores atuais de cobrança e entrega de eventos.
        </p>
      </header>
      {operations.isLoading && <QueryLoading />}
      {operations.error && <QueryError error={operations.error} />}
      {operations.data && (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {metrics.map(({ key, label, icon: Icon, detail }) => (
            <Card key={key}>
              <CardHeader>
                <CardDescription className="flex items-center gap-2">
                  <Icon aria-hidden="true" className="size-4" />
                  {label}
                </CardDescription>
                <CardTitle className="text-3xl tabular-nums">
                  {operations.data[key].toLocaleString("pt-BR")}
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-xs text-muted-foreground">{detail}</p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
      <Card>
        <CardHeader>
          <CardTitle>Investigar por workspace</CardTitle>
          <CardDescription>
            Consulte planos, carteira, créditos e consumo de um workspace.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Link
            to="/workspaces"
            className="inline-flex items-center gap-2 text-sm font-medium text-primary hover:underline"
          >
            Abrir workspaces <ArrowRight className="size-4" />
          </Link>
        </CardContent>
      </Card>
    </div>
  );
}
