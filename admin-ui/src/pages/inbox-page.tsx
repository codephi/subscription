import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { listIntegrationInbox, replayInbox } from "@/api/operations-api";
import { CursorPager } from "@/components/cursor-pager";
import {
  QueryEmpty,
  QueryError,
  QueryLoading,
} from "@/components/query-feedback";
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

export function InboxPage() {
  const [workspace, setWorkspace] = useState("");
  const [status, setStatus] = useState("all");
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const valid = !workspace.trim() || uuidPattern.test(workspace.trim());
  const page = useQuery({
    queryKey: ["inbox", cursor, workspace, status],
    queryFn: () =>
      listIntegrationInbox(
        cursor,
        workspace.trim() || undefined,
        status === "all" ? undefined : status,
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
        <p className="text-sm text-muted-foreground">INTEGRAÇÕES · ACCOUNTS</p>
        <h1 className="text-3xl font-semibold tracking-tight">Inbox</h1>
        <p className="text-muted-foreground">
          Eventos recebidos e estado de processamento.
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>Filtros</CardTitle>
          <CardDescription>
            Localize eventos por workspace e estado.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup className="grid gap-4 md:grid-cols-2">
            <Field data-invalid={!valid}>
              <FieldLabel htmlFor="inbox-workspace">Workspace ID</FieldLabel>
              <Input
                id="inbox-workspace"
                value={workspace}
                onChange={(event) => {
                  setWorkspace(event.target.value);
                  resetPage();
                }}
                aria-invalid={!valid}
                placeholder="Todos"
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="inbox-status">Estado</FieldLabel>
              <Select
                value={status}
                onValueChange={(value) => {
                  setStatus(value ?? "all");
                  resetPage();
                }}
              >
                <SelectTrigger id="inbox-status" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {[
                      "all",
                      "RECEIVED",
                      "PROCESSED",
                      "IGNORED",
                      "QUARANTINED",
                    ].map((value) => (
                      <SelectItem key={value} value={value}>
                        {value === "all" ? "Todos" : value}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
          </FieldGroup>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Eventos</CardTitle>
          <CardDescription>
            A repetição reaplica as regras de processamento da API.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {!valid && (
            <p role="alert" className="text-sm text-destructive">
              Workspace deve ser um UUID válido.
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
                    <TableHead>Evento</TableHead>
                    <TableHead>Workspace</TableHead>
                    <TableHead>Tipo</TableHead>
                    <TableHead>Sequência</TableHead>
                    <TableHead>Estado</TableHead>
                    <TableHead>Recebido</TableHead>
                    <TableHead>Ação</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.data.items.map((event) => (
                    <InboxRow key={event.event_id} event={event} />
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

function InboxRow({
  event,
}: {
  event: Awaited<ReturnType<typeof listIntegrationInbox>>["items"][number];
}) {
  const client = useQueryClient();
  const replay = useMutation({
    mutationFn: () => replayInbox(event.event_id),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["inbox"] });
    },
  });
  return (
    <TableRow>
      <TableCell className="font-mono text-xs" title={event.event_id}>
        {shortId(event.event_id)}
      </TableCell>
      <TableCell>
        <Link
          className="font-mono text-xs text-primary hover:underline"
          to={`/workspaces/${event.workspace_id}`}
        >
          {shortId(event.workspace_id)}
        </Link>
      </TableCell>
      <TableCell>{event.event_type}</TableCell>
      <TableCell>{event.external_sequence}</TableCell>
      <TableCell>
        <Badge variant="outline">{event.processing_status}</Badge>
      </TableCell>
      <TableCell>{formatDate(event.received_at)}</TableCell>
      <TableCell>
        {event.processing_status === "QUARANTINED" && (
          <AlertDialog>
            <AlertDialogTrigger render={<Button variant="outline" size="sm" />}>
              Repetir
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Repetir evento?</AlertDialogTitle>
                <AlertDialogDescription>
                  O evento {event.event_id} será reprocessado pelas regras
                  atuais.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Voltar</AlertDialogCancel>
                <AlertDialogAction
                  disabled={replay.isPending}
                  onClick={() => replay.mutate()}
                >
                  Confirmar
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
        {replay.error && <QueryError error={replay.error} />}
      </TableCell>
    </TableRow>
  );
}
