import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { listAuditEvents } from "@/api/operations-api";
import { CursorPager } from "@/components/cursor-pager";
import {
  QueryEmpty,
  QueryError,
  QueryLoading,
} from "@/components/query-feedback";
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
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatDate, shortId } from "@/lib/format";

const uuidPattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function AuditPage() {
  const [account, setAccount] = useState("");
  const [action, setAction] = useState("");
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const valid = !account.trim() || uuidPattern.test(account.trim());
  const page = useQuery({
    queryKey: ["audit", cursor, account, action],
    queryFn: () =>
      listAuditEvents(
        cursor,
        account.trim() || undefined,
        action.trim() || undefined,
      ),
    enabled: valid,
  });
  const resetPage = () => {
    setCursor(undefined);
    setHistory([]);
  };
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <p className="text-sm text-muted-foreground">OPERAÇÃO</p>
        <h1 className="text-3xl font-semibold tracking-tight">Auditoria</h1>
        <p className="text-muted-foreground">
          Eventos registrados pela API. A referência de ator é texto da
          operação, não uma identidade autenticada.
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>Filtros</CardTitle>
          <CardDescription>
            Use account e ação para localizar evidências.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup className="grid gap-4 md:grid-cols-2">
            <Field data-invalid={!valid}>
              <FieldLabel htmlFor="audit-account">Account ID</FieldLabel>
              <Input
                id="audit-account"
                value={account}
                onChange={(event) => {
                  setAccount(event.target.value);
                  resetPage();
                }}
                placeholder="Todos"
                aria-invalid={!valid}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="audit-action">Ação</FieldLabel>
              <Input
                id="audit-action"
                value={action}
                onChange={(event) => {
                  setAction(event.target.value);
                  resetPage();
                }}
                placeholder="Todas"
              />
            </Field>
          </FieldGroup>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Eventos</CardTitle>
          <CardDescription>Ordenação estável por ID.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {!valid && (
            <p role="alert" className="text-sm text-destructive">
              Account deve ser um UUID válido.
            </p>
          )}
          {page.isLoading && <QueryLoading />}
          {page.error && <QueryError error={page.error} />}
          {page.data?.items.length === 0 && (
            <QueryEmpty
              title="Sem eventos"
              description="Nenhum evento corresponde aos filtros."
            />
          )}
          {page.data && page.data.items.length > 0 && (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Quando</TableHead>
                    <TableHead>Ação</TableHead>
                    <TableHead>Account</TableHead>
                    <TableHead>Recurso</TableHead>
                    <TableHead>Correlação</TableHead>
                    <TableHead>Referência</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.data.items.map((entry) => (
                    <TableRow key={entry.audit_event_id}>
                      <TableCell>{formatDate(entry.occurred_at)}</TableCell>
                      <TableCell>{entry.action}</TableCell>
                      <TableCell>
                        {entry.account_id ? (
                          <Link
                            className="font-mono text-xs text-primary hover:underline"
                            to={`/accounts/${entry.account_id}`}
                          >
                            {shortId(entry.account_id)}
                          </Link>
                        ) : (
                          "—"
                        )}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {entry.resource_kind}{" "}
                        {entry.resource_id ? shortId(entry.resource_id) : ""}
                      </TableCell>
                      <TableCell
                        className="font-mono text-xs"
                        title={entry.correlation_id}
                      >
                        {shortId(entry.correlation_id)}
                      </TableCell>
                      <TableCell className="text-xs">
                        {entry.actor_reference ?? "—"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
          <CursorPager
            canGoBack={history.length > 0}
            nextCursor={page.data?.next_cursor}
            onBack={() => {
              setCursor(history.at(-1));
              setHistory(history.slice(0, -1));
            }}
            onNext={() => {
              setHistory([...history, cursor]);
              setCursor(page.data?.next_cursor ?? undefined);
            }}
          />
        </CardContent>
      </Card>
    </div>
  );
}
