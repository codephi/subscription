import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import { listCatalogEntries } from "@/api/catalog-api";
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
import {
  catalogKinds,
  parseCatalogKind,
  type CatalogKind,
} from "@/lib/catalog-kinds";
import { formatDate, shortId } from "@/lib/format";

export function CatalogListPage() {
  const { kind: rawKind } = useParams();
  const kind = parseCatalogKind(rawKind);
  if (!kind) return <p>Tipo de catálogo desconhecido.</p>;
  return <CatalogEntries key={kind} kind={kind} />;
}

function CatalogEntries({ kind }: { kind: CatalogKind }) {
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const page = useQuery({
    queryKey: ["catalog", kind, cursor],
    queryFn: () => listCatalogEntries(kind, cursor),
  });
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <p className="text-sm text-muted-foreground">CATÁLOGO · OFERTAS</p>
        <h1 className="text-3xl font-semibold tracking-tight">
          {catalogKinds[kind]}
        </h1>
        <p className="text-muted-foreground">
          Versões publicadas são imutáveis; novas versões têm IDs próprios.
        </p>
      </header>
      <nav aria-label="Tipos de catálogo" className="flex flex-wrap gap-2">
        {Object.entries(catalogKinds).map(([slug, label]) => (
          <Button
            key={slug}
            variant={slug === kind ? "secondary" : "outline"}
            size="sm"
            nativeButton={false}
            render={<Link to={`/catalog/${slug}`} />}
          >
            {label}
          </Button>
        ))}
      </nav>
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <CardTitle>Registros</CardTitle>
              <CardDescription>
                Abra um registro para conferir o contrato publicado.
              </CardDescription>
            </div>
            <Button
              nativeButton={false}
              render={<Link to={`/catalog/${kind}/new`} />}
            >
              <Plus data-icon="inline-start" />
              Criar
            </Button>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {page.isLoading && <QueryLoading />}
          {page.error && <QueryError error={page.error} />}
          {page.data?.items.length === 0 && (
            <QueryEmpty
              title="Nenhum registro"
              description="Esta parte do catálogo ainda está vazia."
            />
          )}
          {page.data && page.data.items.length > 0 && (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Nome</TableHead>
                    <TableHead>ID</TableHead>
                    <TableHead>Vínculo</TableHead>
                    <TableHead>Estado</TableHead>
                    <TableHead>Criado</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.data.items.map((entry) => (
                    <TableRow key={entry.id}>
                      <TableCell>
                        <Link
                          className="text-primary hover:underline"
                          to={`/catalog/${kind}/${entry.id}`}
                        >
                          {entry.name}
                        </Link>
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {shortId(entry.id)}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {entry.parent_id ? shortId(entry.parent_id) : "—"}
                      </TableCell>
                      <TableCell>
                        <Badge variant="outline">{entry.status}</Badge>
                      </TableCell>
                      <TableCell>{formatDate(entry.created_at)}</TableCell>
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
