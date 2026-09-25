import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { listBillingRecords } from "@/api/billing-api";
import { CursorPager } from "@/components/cursor-pager";
import {
  QueryEmpty,
  QueryError,
  QueryLoading,
} from "@/components/query-feedback";
import { Badge } from "@/components/ui/badge";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  billingKinds,
  billingStatuses,
  parseBillingKind,
} from "@/lib/billing-records";
import { formatDate, shortId } from "@/lib/format";
import { usePanelStore } from "@/store/panel-store";

const uuidPattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function BillingListPage() {
  const { kind: rawKind } = useParams();
  const [searchParams] = useSearchParams();
  const kind = parseBillingKind(rawKind);
  if (!kind) return <p>Fila de Billing desconhecida.</p>;
  return (
    <BillingRecords key={`${kind}:${searchParams.toString()}`} kind={kind} />
  );
}

function BillingRecords({ kind }: { kind: keyof typeof billingKinds }) {
  const [searchParams, setSearchParams] = useSearchParams();
  const workspace = usePanelStore((state) => state.billingWorkspaceFilter);
  const setWorkspace = usePanelStore(
    (state) => state.setBillingWorkspaceFilter,
  );
  const setStatus = usePanelStore((state) => state.setBillingStatusFilter);
  const status = searchParams.get("status") ?? "all";
  const collectionId = searchParams.get("collection_request_id") ?? "";
  const correlationId = searchParams.get("correlation_id") ?? "";
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const validWorkspace =
    workspace.trim() === "" || uuidPattern.test(workspace.trim());
  const validReferences =
    (!collectionId || uuidPattern.test(collectionId)) &&
    (!correlationId || uuidPattern.test(correlationId));
  const selectedStatus = billingStatuses(kind).includes(status)
    ? status
    : "all";
  const updateReference = (name: string, value: string) => {
    const updated = new URLSearchParams(searchParams);
    if (value) updated.set(name, value);
    else updated.delete(name);
    setSearchParams(updated);
  };
  const page = useQuery({
    queryKey: [
      "billing-records",
      kind,
      cursor,
      workspace,
      selectedStatus,
      collectionId,
      correlationId,
    ],
    queryFn: () =>
      listBillingRecords(
        kind,
        cursor,
        workspace.trim() || undefined,
        selectedStatus === "all" ? undefined : selectedStatus,
        collectionId || undefined,
        correlationId || undefined,
      ),
    enabled: validWorkspace && validReferences,
  });
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <p className="text-sm text-muted-foreground">INVESTIGAÇÃO · BILLING</p>
        <h1 className="text-3xl font-semibold tracking-tight">
          {billingKinds[kind]}
        </h1>
        <p className="text-muted-foreground">
          Consulte estado, referências e horário dos registros.
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>Filtros</CardTitle>
          <CardDescription>Workspace e estado dos registros.</CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup className="grid gap-4 md:grid-cols-2">
            <Field data-invalid={!validWorkspace}>
              <FieldLabel htmlFor="billing-workspace">Workspace ID</FieldLabel>
              <Input
                id="billing-workspace"
                value={workspace}
                onChange={(event) => {
                  setWorkspace(event.target.value);
                  setCursor(undefined);
                  setHistory([]);
                }}
                placeholder="Todos os workspaces"
                aria-invalid={!validWorkspace}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="billing-status">Estado</FieldLabel>
              <Select
                value={selectedStatus}
                onValueChange={(value) => {
                  const next = value ?? "all";
                  setStatus(next);
                  const updated = new URLSearchParams(searchParams);
                  if (next === "all") updated.delete("status");
                  else updated.set("status", next);
                  setSearchParams(updated);
                }}
              >
                <SelectTrigger id="billing-status" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="all">Todos</SelectItem>
                    {billingStatuses(kind).map((value) => (
                      <SelectItem key={value} value={value}>
                        {value}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            <Field
              data-invalid={!!collectionId && !uuidPattern.test(collectionId)}
            >
              <FieldLabel htmlFor="billing-collection">Cobrança ID</FieldLabel>
              <Input
                id="billing-collection"
                value={collectionId}
                onChange={(event) =>
                  updateReference("collection_request_id", event.target.value)
                }
                placeholder="Todas"
                aria-invalid={!!collectionId && !uuidPattern.test(collectionId)}
              />
            </Field>
            <Field
              data-invalid={!!correlationId && !uuidPattern.test(correlationId)}
            >
              <FieldLabel htmlFor="billing-correlation">
                Correlação ID
              </FieldLabel>
              <Input
                id="billing-correlation"
                value={correlationId}
                onChange={(event) =>
                  updateReference("correlation_id", event.target.value)
                }
                placeholder="Todas"
                aria-invalid={
                  !!correlationId && !uuidPattern.test(correlationId)
                }
              />
            </Field>
          </FieldGroup>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Registros</CardTitle>
          <CardDescription>
            Ordenados por ID estável; abra um registro para correlacionar as
            evidências.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {!validWorkspace && (
            <p role="alert" className="text-sm text-destructive">
              Informe um UUID válido ou deixe o filtro vazio.
            </p>
          )}
          {!validReferences && (
            <p role="alert" className="text-sm text-destructive">
              A referência de correlação deve ser um UUID válido.
            </p>
          )}
          {page.isLoading && <QueryLoading />}
          {page.error && <QueryError error={page.error} />}
          {page.data?.items.length === 0 && (
            <QueryEmpty
              title="Nenhum registro"
              description="Não há registros para estes filtros."
            />
          )}
          {page.data && page.data.items.length > 0 && (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>ID</TableHead>
                    <TableHead>Estado</TableHead>
                    <TableHead>Workspace</TableHead>
                    <TableHead>Referência</TableHead>
                    <TableHead>Recebido/criado</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.data.items.map((record) => (
                    <TableRow key={record.id}>
                      <TableCell>
                        <Link
                          className="font-mono text-xs text-primary hover:underline"
                          to={`/billing/${kind}/${record.id}`}
                        >
                          {shortId(record.id)}
                        </Link>
                      </TableCell>
                      <TableCell>
                        <Badge variant="outline">{record.status}</Badge>
                      </TableCell>
                      <TableCell>
                        {record.workspace_id ? (
                          <Link
                            className="font-mono text-xs text-primary hover:underline"
                            to={`/workspaces/${record.workspace_id}`}
                          >
                            {shortId(record.workspace_id)}
                          </Link>
                        ) : (
                          "—"
                        )}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {record.collection_request_id
                          ? shortId(record.collection_request_id)
                          : record.provider_event_id
                            ? shortId(record.provider_event_id)
                            : "—"}
                      </TableCell>
                      <TableCell>{formatDate(record.occurred_at)}</TableCell>
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
