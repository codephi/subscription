import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import { useEffect } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getProvisioning,
  getWallets,
  getWorkspace,
  reconcileProvisioning,
} from "@/api/client";
import { QueryError, QueryLoading } from "@/components/query-feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { formatDate, formatUnits } from "@/lib/format";
import { usePanelStore } from "@/store/panel-store";
import {
  CreditsPanel,
  ItemUsagePanel,
  PlansPanel,
} from "@/pages/workspace-panels";

export function WorkspacePage() {
  const { workspaceId } = useParams<{ workspaceId: string }>();
  if (!workspaceId) return <p>Workspace ID ausente.</p>;
  return <WorkspaceContent key={workspaceId} workspaceId={workspaceId} />;
}

function WorkspaceContent({ workspaceId }: { workspaceId: string }) {
  const queryClient = useQueryClient();
  const selectItem = usePanelStore((state) => state.selectItem);
  const workspace = useQuery({
    queryKey: ["workspace", workspaceId],
    queryFn: () => getWorkspace(workspaceId),
  });
  const wallets = useQuery({
    queryKey: ["wallets", workspaceId],
    queryFn: () => getWallets(workspaceId),
  });
  const provisioning = useQuery({
    queryKey: ["provisioning", workspaceId],
    queryFn: () => getProvisioning(workspaceId),
  });
  const provisionWallets = useMutation({
    mutationFn: () => reconcileProvisioning(workspaceId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["provisioning", workspaceId],
      });
      await queryClient.invalidateQueries({
        queryKey: ["wallets", workspaceId],
      });
    },
  });
  useEffect(() => selectItem(null), [workspaceId, selectItem]);
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-3">
        <Link
          to="/workspaces"
          className="inline-flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="size-4" />
          Workspaces
        </Link>
        <div>
          <p className="text-sm font-medium text-muted-foreground">
            DETALHE OPERACIONAL
          </p>
          <h1 className="break-all text-2xl font-semibold tracking-tight md:text-3xl">
            {workspaceId}
          </h1>
        </div>
        <Button
          variant="outline"
          nativeButton={false}
          render={<Link to={`/workspaces/${workspaceId}/actions`} />}
        >
          Ações administrativas
        </Button>
        <Button
          variant="outline"
          nativeButton={false}
          render={<Link to={`/workspaces/${workspaceId}/integrations`} />}
        >
          Integrações
        </Button>
      </header>
      {workspace.isLoading && <QueryLoading />}
      {workspace.error && <QueryError error={workspace.error} />}
      {workspace.data && (
        <div className="grid gap-4 md:grid-cols-3">
          <Card>
            <CardHeader>
              <CardDescription>Estado do workspace</CardDescription>
              <CardTitle>
                <Badge variant="outline">
                  {workspace.data.operational_status}
                </Badge>
              </CardTitle>
            </CardHeader>
            <CardContent className="text-xs text-muted-foreground">
              Sequência externa {workspace.data.external_sequence} · atualizado{" "}
              {formatDate(workspace.data.updated_at)}
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardDescription>Saldo de créditos</CardDescription>
              <CardTitle className="text-2xl tabular-nums">
                {formatUnits(
                  wallets.data?.customer_wallet.balance_credit_units,
                )}
              </CardTitle>
            </CardHeader>
            <CardContent className="text-xs text-muted-foreground">
              {wallets.isLoading
                ? "Carregando carteira…"
                : wallets.error
                  ? "Carteira indisponível"
                  : "Unidades de crédito disponíveis"}
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardDescription>Provisionamento</CardDescription>
              <CardTitle>
                <Badge variant="outline">
                  {provisioning.data?.status ?? "Indisponível"}
                </Badge>
              </CardTitle>
            </CardHeader>
            <CardContent className="text-xs text-muted-foreground">
              {provisioning.data
                ? `${provisioning.data.materialized_item_wallets} de ${provisioning.data.expected_item_wallets} item wallets`
                : provisioning.isLoading
                  ? "Carregando…"
                  : "Sem estado disponível"}
              <p className="mt-2">
                Materializa a carteira do workspace e as item-wallets do
                catálogo atual. A operação pode ser repetida com segurança.
              </p>
              <Button
                className="mt-4"
                variant="outline"
                disabled={provisionWallets.isPending}
                onClick={() => provisionWallets.mutate()}
              >
                {provisionWallets.isPending
                  ? "Provisionando…"
                  : "Provisionar wallet"}
              </Button>
              {provisionWallets.error && (
                <div className="mt-3">
                  <QueryError error={provisionWallets.error} />
                </div>
              )}
              {provisionWallets.data && (
                <p role="status" className="mt-3 text-foreground">
                  Provisionamento: {provisionWallets.data.status} (
                  {provisionWallets.data.materialized_item_wallets}/
                  {provisionWallets.data.expected_item_wallets} item wallets).
                </p>
              )}
            </CardContent>
          </Card>
        </div>
      )}
      {workspace.data && (wallets.error || provisioning.error) && (
        <div className="grid gap-3">
          {wallets.error && <QueryError error={wallets.error} />}
          {provisioning.error && <QueryError error={provisioning.error} />}
        </div>
      )}
      {workspace.data && <PlansPanel workspaceId={workspaceId} />}
      {workspace.data && <CreditsPanel workspaceId={workspaceId} />}
      {workspace.data && wallets.isLoading && <QueryLoading />}
      {workspace.data && wallets.data && (
        <ItemUsagePanel
          workspaceId={workspaceId}
          items={wallets.data.item_wallets}
        />
      )}
    </div>
  );
}
