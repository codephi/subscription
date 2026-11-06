import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import { useEffect } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getProvisioning,
  getWallets,
  getAccount,
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
} from "@/pages/account-panels";

export function AccountPage() {
  const { accountId } = useParams<{ accountId: string }>();
  if (!accountId) return <p>Account ID ausente.</p>;
  return <AccountContent key={accountId} accountId={accountId} />;
}

function AccountContent({ accountId }: { accountId: string }) {
  const queryClient = useQueryClient();
  const selectItem = usePanelStore((state) => state.selectItem);
  const account = useQuery({
    queryKey: ["account", accountId],
    queryFn: () => getAccount(accountId),
  });
  const wallets = useQuery({
    queryKey: ["wallets", accountId],
    queryFn: () => getWallets(accountId),
  });
  const provisioning = useQuery({
    queryKey: ["provisioning", accountId],
    queryFn: () => getProvisioning(accountId),
  });
  const provisionWallets = useMutation({
    mutationFn: () => reconcileProvisioning(accountId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["provisioning", accountId],
      });
      await queryClient.invalidateQueries({
        queryKey: ["wallets", accountId],
      });
    },
  });
  useEffect(() => selectItem(null), [accountId, selectItem]);
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-3">
        <Link
          to="/accounts"
          className="inline-flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="size-4" />
          Accounts
        </Link>
        <div>
          <p className="text-sm font-medium text-muted-foreground">
            DETALHE OPERACIONAL
          </p>
          <h1 className="break-all text-2xl font-semibold tracking-tight md:text-3xl">
            {accountId}
          </h1>
        </div>
        <Button
          variant="outline"
          nativeButton={false}
          render={<Link to={`/accounts/${accountId}/actions`} />}
        >
          Ações administrativas
        </Button>
        <Button
          variant="outline"
          nativeButton={false}
          render={<Link to={`/accounts/${accountId}/integrations`} />}
        >
          Integrações
        </Button>
      </header>
      {account.isLoading && <QueryLoading />}
      {account.error && <QueryError error={account.error} />}
      {account.data && (
        <div className="grid gap-4 md:grid-cols-3">
          <Card>
            <CardHeader>
              <CardDescription>Estado do account</CardDescription>
              <CardTitle>
                <Badge variant="outline">
                  {account.data.operational_status}
                </Badge>
              </CardTitle>
            </CardHeader>
            <CardContent className="text-xs text-muted-foreground">
              Sequência externa {account.data.external_sequence} · atualizado{" "}
              {formatDate(account.data.updated_at)}
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
                Materializa a carteira do account e as item-wallets do
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
      {account.data && (wallets.error || provisioning.error) && (
        <div className="grid gap-3">
          {wallets.error && <QueryError error={wallets.error} />}
          {provisioning.error && <QueryError error={provisioning.error} />}
        </div>
      )}
      {account.data && <PlansPanel accountId={accountId} />}
      {account.data && <CreditsPanel accountId={accountId} />}
      {account.data && wallets.isLoading && <QueryLoading />}
      {account.data && wallets.data && (
        <ItemUsagePanel
          accountId={accountId}
          items={wallets.data.item_wallets}
        />
      )}
    </div>
  );
}
