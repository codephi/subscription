import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  createWorkspace,
  getWorkspace,
  listWorkspaces,
  terminateWorkspace,
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
import { formatDate } from "@/lib/format";
import { usePanelStore } from "@/store/panel-store";

const uuidPattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function WorkspaceListPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const search = usePanelStore((state) => state.workspaceSearch);
  const setSearch = usePanelStore((state) => state.setWorkspaceSearch);
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const [lookupId, setLookupId] = useState<string>();
  const [actorReference, setActorReference] = useState("");
  const termination = useMutation({
    mutationFn: (workspaceId: string) => terminateWorkspace(workspaceId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["workspaces"] });
    },
  });
  const creation = useMutation({
    mutationFn: () => createWorkspace(actorReference.trim()),
    onSuccess: async (workspace) => {
      await queryClient.invalidateQueries({ queryKey: ["workspaces"] });
      navigate(`/workspaces/${workspace.workspace_id}`);
    },
  });
  const page = useQuery({
    queryKey: ["workspaces", cursor],
    queryFn: () => listWorkspaces(cursor),
  });
  const lookup = useQuery({
    queryKey: ["workspace-lookup", lookupId],
    queryFn: () => getWorkspace(lookupId!),
    enabled: !!lookupId,
    retry: false,
  });

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!uuidPattern.test(search.trim())) return;
    if (lookupId === search.trim()) {
      void lookup.refetch();
      return;
    }
    setLookupId(search.trim());
  }

  useEffect(() => {
    if (lookup.data) navigate(`/workspaces/${lookup.data.workspace_id}`);
  }, [lookup.data, navigate]);

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <p className="text-sm font-medium text-muted-foreground">
          ADMINISTRAÇÃO
        </p>
        <h1 className="text-3xl font-semibold tracking-tight">Workspaces</h1>
        <p className="text-muted-foreground">
          Workspaces recebidos do Accounts ou criados pela administração.
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>Criar workspace</CardTitle>
          <CardDescription>
            Gera um novo ID e registra o workspace no estado inicial CREATED.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              creation.mutate();
            }}
            className="flex flex-col gap-3 sm:flex-row sm:items-end"
          >
            <FieldGroup className="flex-1">
              <Field>
                <FieldLabel htmlFor="workspace-actor">Criado por</FieldLabel>
                <Input
                  id="workspace-actor"
                  value={actorReference}
                  onChange={(event) => setActorReference(event.target.value)}
                  placeholder="operador ou e-mail"
                  maxLength={255}
                />
              </Field>
            </FieldGroup>
            <Button
              type="submit"
              disabled={!actorReference.trim() || creation.isPending}
            >
              <Plus data-icon="inline-start" />
              {creation.isPending ? "Criando…" : "Criar workspace"}
            </Button>
          </form>
          {creation.error && <QueryError error={creation.error} />}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Abrir por ID</CardTitle>
          <CardDescription>
            Informe o UUID completo de um workspace.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form
            onSubmit={submitSearch}
            className="flex flex-col gap-3 sm:flex-row sm:items-end"
          >
            <FieldGroup className="flex-1">
              <Field
                data-invalid={
                  search.length > 0 && !uuidPattern.test(search.trim())
                }
              >
                <FieldLabel htmlFor="workspace-id">Workspace ID</FieldLabel>
                <Input
                  id="workspace-id"
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                  aria-invalid={
                    search.length > 0 && !uuidPattern.test(search.trim())
                  }
                />
              </Field>
            </FieldGroup>
            <Button
              type="submit"
              disabled={!uuidPattern.test(search.trim()) || lookup.isFetching}
            >
              <Search data-icon="inline-start" />
              Buscar
            </Button>
          </form>
          {lookup.error && (
            <div className="mt-4">
              <QueryError error={lookup.error} />
            </div>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Workspaces registrados</CardTitle>
          <CardDescription>
            IDs e estados conhecidos por este serviço.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {page.isLoading && <QueryLoading />}
          {page.error && <QueryError error={page.error} />}
          {page.data?.items.length === 0 && (
            <QueryEmpty
              title="Nenhum workspace"
              description="Ainda não há projeções disponíveis nesta página."
            />
          )}
          {page.data && page.data.items.length > 0 && (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Workspace ID</TableHead>
                    <TableHead>Estado</TableHead>
                    <TableHead>Sequência</TableHead>
                    <TableHead>Atualizado</TableHead>
                    <TableHead>Ações</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.data.items.map((workspace) => (
                    <TableRow key={workspace.workspace_id}>
                      <TableCell>
                        <Link
                          to={`/workspaces/${workspace.workspace_id}`}
                          className="font-mono text-xs text-primary hover:underline"
                        >
                          {workspace.workspace_id}
                        </Link>
                      </TableCell>
                      <TableCell>
                        <Badge variant="outline">
                          {workspace.operational_status}
                        </Badge>
                      </TableCell>
                      <TableCell>{workspace.external_sequence}</TableCell>
                      <TableCell>{formatDate(workspace.updated_at)}</TableCell>
                      <TableCell>
                        <AlertDialog>
                          <AlertDialogTrigger
                            render={<Button variant="outline" size="sm" />}
                            disabled={
                              workspace.operational_status === "TERMINATED" ||
                              termination.isPending
                            }
                          >
                            <Trash2 data-icon="inline-start" />
                            Apagar
                          </AlertDialogTrigger>
                          <AlertDialogContent>
                            <AlertDialogHeader>
                              <AlertDialogTitle>
                                Apagar este workspace?
                              </AlertDialogTitle>
                              <AlertDialogDescription>
                                O workspace será encerrado e deixará de aceitar
                                operações. Planos, cobranças, carteiras e
                                auditoria serão preservados. Esta ação não pode
                                ser desfeita.
                              </AlertDialogDescription>
                            </AlertDialogHeader>
                            <AlertDialogFooter>
                              <AlertDialogCancel>Voltar</AlertDialogCancel>
                              <AlertDialogAction
                                onClick={() =>
                                  termination.mutate(workspace.workspace_id)
                                }
                              >
                                Encerrar workspace
                              </AlertDialogAction>
                            </AlertDialogFooter>
                          </AlertDialogContent>
                        </AlertDialog>
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
          {termination.error && <QueryError error={termination.error} />}
        </CardContent>
      </Card>
    </div>
  );
}
