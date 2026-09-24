import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import {
  getItemMeter,
  getItemStatement,
  getStatement,
  listCustomerPlans,
} from "@/api/client";
import { CursorPager } from "@/components/cursor-pager";
import {
  QueryEmpty,
  QueryError,
  QueryLoading,
} from "@/components/query-feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { components } from "@/api/generated";
import { formatDate, formatUnits, shortId } from "@/lib/format";
import { usePanelStore } from "@/store/panel-store";

type ItemWallet = components["schemas"]["ItemWalletResponse"];

/** Show customer plan states and current cycles; e.g. `<PlansPanel workspaceId={id} />`. */
export function PlansPanel({ workspaceId }: { workspaceId: string }) {
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const plans = useQuery({
    queryKey: ["plans", workspaceId, cursor],
    queryFn: () => listCustomerPlans(workspaceId, cursor),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>Planos do cliente</CardTitle>
        <CardDescription>
          Estado comercial, renovação e ciclo atual.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {plans.isLoading && <QueryLoading />}
        {plans.error && <QueryError error={plans.error} />}
        {plans.data?.items.length === 0 && (
          <QueryEmpty
            title="Nenhum plano"
            description="Este workspace ainda não possui planos registrados."
          />
        )}
        {plans.data && plans.data.items.length > 0 && (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Plano</TableHead>
                  <TableHead>Estado</TableHead>
                  <TableHead>Renovação</TableHead>
                  <TableHead>Ciclo atual</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {plans.data.items.map((plan) => (
                  <TableRow key={plan.customer_plan_id}>
                    <TableCell>
                      <div className="flex flex-col">
                        <span
                          className="font-mono text-xs"
                          title={plan.customer_plan_id}
                        >
                          {shortId(plan.customer_plan_id)}
                        </span>
                        <span
                          className="text-xs text-muted-foreground"
                          title={plan.plan_version_id}
                        >
                          Versão {shortId(plan.plan_version_id)}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{plan.commercial_status}</Badge>
                      <p className="mt-1 text-xs text-muted-foreground">
                        {plan.activation_status}
                      </p>
                    </TableCell>
                    <TableCell>
                      <Badge variant="secondary">{plan.renewal_status}</Badge>
                      {plan.cancel_at_period_end && (
                        <p className="mt-1 text-xs">Cancelará ao fim</p>
                      )}
                    </TableCell>
                    <TableCell className="text-xs">
                      {plan.current_cycle ? (
                        <>
                          <span>
                            #{plan.current_cycle.cycle_ordinal} ·{" "}
                            {plan.current_cycle.status}
                          </span>
                          <br />
                          <span>
                            {formatDate(
                              plan.current_cycle.current_period_start,
                            )}{" "}
                            →{" "}
                            {formatDate(plan.current_cycle.current_period_end)}
                          </span>
                        </>
                      ) : (
                        "Sem ciclo atual"
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
        <CursorPager
          canGoBack={history.length > 0}
          nextCursor={plans.data?.next_cursor}
          onBack={() => {
            setCursor(history.at(-1));
            setHistory(history.slice(0, -1));
          }}
          onNext={() => {
            setHistory([...history, cursor]);
            setCursor(plans.data?.next_cursor ?? undefined);
          }}
        />
      </CardContent>
    </Card>
  );
}

/** Show the immutable customer credit statement; e.g. `<CreditsPanel workspaceId={id} />`. */
export function CreditsPanel({ workspaceId }: { workspaceId: string }) {
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const statement = useQuery({
    queryKey: ["credits", workspaceId, cursor],
    queryFn: () => getStatement(workspaceId, cursor),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>Extrato de créditos</CardTitle>
        <CardDescription>
          Lançamentos persistidos na carteira do cliente.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {statement.isLoading && <QueryLoading />}
        {statement.error && <QueryError error={statement.error} />}
        {statement.data?.items.length === 0 && (
          <QueryEmpty
            title="Sem lançamentos"
            description="Nenhum crédito ou débito foi registrado nesta página."
          />
        )}
        {statement.data && statement.data.items.length > 0 && (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Data</TableHead>
                  <TableHead>Tipo</TableHead>
                  <TableHead>Origem</TableHead>
                  <TableHead>Unidades</TableHead>
                  <TableHead>Saldo após</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {statement.data.items.map((entry) => (
                  <TableRow key={entry.customer_wallet_entry_id}>
                    <TableCell className="text-xs">
                      {formatDate(entry.created_at)}
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{entry.entry_type}</Badge>
                    </TableCell>
                    <TableCell className="text-xs">
                      {entry.source_channel}
                    </TableCell>
                    <TableCell className="tabular-nums">
                      {formatUnits(entry.signed_credit_units)}
                    </TableCell>
                    <TableCell className="tabular-nums">
                      {formatUnits(entry.balance_after_credit_units)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
        <CursorPager
          canGoBack={history.length > 0}
          nextCursor={statement.data?.next_cursor}
          onBack={() => {
            setCursor(history.at(-1));
            setHistory(history.slice(0, -1));
          }}
          onNext={() => {
            setHistory([...history, cursor]);
            setCursor(statement.data?.next_cursor ?? undefined);
          }}
        />
      </CardContent>
    </Card>
  );
}

/** Load an item meter only after selection; e.g. `<ItemUsagePanel workspaceId={id} items={wallets} />`. */
export function ItemUsagePanel({
  workspaceId,
  items,
}: {
  workspaceId: string;
  items: ItemWallet[];
}) {
  const selected = usePanelStore((state) => state.selectedItemId);
  const selectItem = usePanelStore((state) => state.selectItem);
  const activeItem = items.some((item) => item.item_id === selected)
    ? selected
    : null;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Consumo por item</CardTitle>
        <CardDescription>
          Selecione uma item wallet para carregar medidor e lançamentos de uso.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        {items.length === 0 && (
          <QueryEmpty
            title="Sem itens"
            description="Este workspace não possui item wallets materializadas."
          />
        )}
        {items.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {items.map((item) => (
              <Button
                key={item.item_id}
                variant={activeItem === item.item_id ? "secondary" : "outline"}
                onClick={() => selectItem(item.item_id)}
                title={item.item_id}
              >
                {shortId(item.item_id)} · {item.status}
              </Button>
            ))}
          </div>
        )}
        {activeItem && (
          <SelectedItemUsage
            key={`${workspaceId}:${activeItem}`}
            workspaceId={workspaceId}
            itemId={activeItem}
          />
        )}
      </CardContent>
    </Card>
  );
}

function SelectedItemUsage({
  workspaceId,
  itemId,
}: {
  workspaceId: string;
  itemId: string;
}) {
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const meter = useQuery({
    queryKey: ["meter", workspaceId, itemId],
    queryFn: () => getItemMeter(workspaceId, itemId),
  });
  const statement = useQuery({
    queryKey: ["usage", workspaceId, itemId, cursor],
    queryFn: () => getItemStatement(workspaceId, itemId, cursor),
  });
  return (
    <div className="flex flex-col gap-4 border-t pt-5">
      {meter.isLoading && <QueryLoading />}
      {meter.error && <QueryError error={meter.error} />}
      {meter.data && (
        <div className="grid gap-3 text-sm sm:grid-cols-3">
          <div>
            <p className="text-muted-foreground">Recebidas</p>
            <p className="text-xl font-semibold tabular-nums">
              {formatUnits(meter.data.total_received_item_units)}
            </p>
          </div>
          <div>
            <p className="text-muted-foreground">Pendentes</p>
            <p className="text-xl font-semibold tabular-nums">
              {formatUnits(meter.data.pending_item_units)}
            </p>
          </div>
          <div>
            <p className="text-muted-foreground">Até o próximo bloco</p>
            <p className="text-xl font-semibold tabular-nums">
              {formatUnits(meter.data.units_until_next_block)}
            </p>
          </div>
        </div>
      )}
      {statement.isLoading && <QueryLoading />}
      {statement.error && <QueryError error={statement.error} />}
      {statement.data?.items.length === 0 && (
        <QueryEmpty
          title="Sem uso registrado"
          description="Nenhum evento de consumo foi registrado nesta página."
        />
      )}
      {statement.data && statement.data.items.length > 0 && (
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Data</TableHead>
                <TableHead>Transação</TableHead>
                <TableHead>Unidades recebidas</TableHead>
                <TableHead>Créditos debitados</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {statement.data.items.map((entry) => (
                <TableRow key={entry.item_wallet_entry_id}>
                  <TableCell className="text-xs">
                    {formatDate(entry.accepted_at)}
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {entry.transaction_id}
                  </TableCell>
                  <TableCell>
                    {formatUnits(entry.received_item_units)}
                  </TableCell>
                  <TableCell>
                    {formatUnits(entry.emitted_debited_credit_units)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
      <CursorPager
        canGoBack={history.length > 0}
        nextCursor={statement.data?.next_cursor}
        onBack={() => {
          setCursor(history.at(-1));
          setHistory(history.slice(0, -1));
        }}
        onNext={() => {
          setHistory([...history, cursor]);
          setCursor(statement.data?.next_cursor ?? undefined);
        }}
      />
    </div>
  );
}
