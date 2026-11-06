import { useState } from "react";
import { Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Plus, TicketPercent } from "lucide-react";
import { listPromotions } from "@/api/promotions-api";
import { CursorPager } from "@/components/cursor-pager";
import { QueryEmpty, QueryError, QueryLoading } from "@/components/query-feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatDate, shortId } from "@/lib/format";

export function PromotionsPage() {
  const [kind, setKind] = useState<"vouchers" | "coupons">("vouchers");
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<string>();
  const [cursor, setCursor] = useState<string>();
  const [history, setHistory] = useState<(string | undefined)[]>([]);
  const page = useQuery({
    queryKey: ["promotions", kind, search, status, cursor],
    queryFn: () => listPromotions(kind, { search: search || undefined, status, cursor, limit: 20 }),
  });
  const createUrl = `/promotions/new/${kind}`;
  return (
    <main className="flex flex-col gap-6">
      <header className="flex flex-col gap-2">
        <p className="text-sm text-muted-foreground">PROMOÇÕES</p>
        <h1 className="text-3xl font-semibold tracking-tight">Vouchers e cupons</h1>
        <p className="text-muted-foreground">Vouchers concedem créditos; cupons reduzem o preço de uma compra.</p>
      </header>
      <div className="flex flex-wrap gap-2">
        <Button variant={kind === "vouchers" ? "secondary" : "outline"} onClick={() => { setKind("vouchers"); setCursor(undefined); setHistory([]); }}>Vouchers</Button>
        <Button variant={kind === "coupons" ? "secondary" : "outline"} onClick={() => { setKind("coupons"); setCursor(undefined); setHistory([]); }}>Cupons</Button>
      </div>
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div><CardTitle>{kind === "vouchers" ? "Vouchers cadastrados" : "Cupons cadastrados"}</CardTitle><CardDescription>Consulte o código, estado e utilizações concluídas ou reservadas.</CardDescription></div>
            <div className="flex flex-wrap gap-2"><Button variant="outline" nativeButton={false} render={<Link to="/promotions/checkout" />}>Iniciar checkout com cupom</Button><Button nativeButton={false} render={<Link to={createUrl} />}><Plus data-icon="inline-start" />Cadastrar {kind === "vouchers" ? "voucher" : "cupom"}</Button></div>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-3 md:grid-cols-[1fr_220px]">
            <Input aria-label="Buscar promoção" placeholder="Buscar código ou nome" value={search} onChange={(event) => { setSearch(event.target.value); setCursor(undefined); setHistory([]); }} />
            <select aria-label="Filtrar estado" className="h-9 rounded-md border bg-background px-3 text-sm" value={status ?? ""} onChange={(event) => { setStatus(event.target.value || undefined); setCursor(undefined); setHistory([]); }}>
              <option value="">Todos os estados</option><option value="ACTIVE">Ativos</option><option value="DISABLED">Desativados</option><option value="ARCHIVED">Arquivados</option>
            </select>
          </div>
          {page.isLoading && <QueryLoading />}{page.error && <QueryError error={page.error} />}
          {page.data?.items.length === 0 && <QueryEmpty title="Nenhuma promoção" description="Cadastre o primeiro voucher ou cupom." />}
          {page.data && page.data.items.length > 0 && (
            <div className="overflow-x-auto">
              <Table><TableHeader><TableRow><TableHead>Código</TableHead><TableHead>Benefício</TableHead><TableHead>Estado</TableHead><TableHead>Usos</TableHead><TableHead>Validade</TableHead></TableRow></TableHeader>
                <TableBody>{page.data.items.map((item) => <TableRow key={item.promotion_id}>
                  <TableCell><Link className="font-mono text-primary hover:underline" to={`/promotions/${kind}/${item.promotion_id}`}>{item.code}</Link><p className="text-xs text-muted-foreground">{item.name} · {shortId(item.promotion_id)}</p></TableCell>
                  <TableCell>{item.promotion_kind === "VOUCHER" ? `${item.credit_units} créditos` : item.discount_kind === "PERCENTAGE" ? `${(item.discount_value ?? 0) / 100}%` : `${item.currency} ${(item.discount_value ?? 0) / 100}`}</TableCell>
                  <TableCell><Badge variant="outline">{item.status}</Badge><p className="mt-1 text-xs text-muted-foreground">{availabilityLabel(item.availability)}</p></TableCell><TableCell>{item.completed_uses}{item.max_total_uses ? ` / ${item.max_total_uses}` : " · sem limite total"}{item.reserved_uses ? ` (+${item.reserved_uses} reservados)` : ""}</TableCell>
                  <TableCell>{item.valid_until ? formatDate(item.valid_until) : "Sem validade"}</TableCell>
                </TableRow>)}</TableBody>
              </Table>
            </div>
          )}
          <CursorPager
            canGoBack={history.length > 0}
            nextCursor={page.data?.next_cursor}
            onBack={() => { setCursor(history.at(-1)); setHistory(history.slice(0, -1)); }}
            onNext={() => { if (!page.data?.next_cursor) return; setHistory([...history, cursor]); setCursor(page.data.next_cursor); }}
          />
        </CardContent>
      </Card>
      <Card><CardContent className="flex items-center gap-3 py-5"><TicketPercent aria-hidden="true" /><p className="text-sm text-muted-foreground">O limite por account começa em 1 uso. Limites vazios são ilimitados.</p></CardContent></Card>
    </main>
  );
}

function availabilityLabel(value: string) {
  const labels: Record<string, string> = {
    AVAILABLE: "Disponível",
    NOT_STARTED: "Ainda não válido",
    EXPIRED: "Expirado",
    EXHAUSTED: "Limite total atingido",
    DISABLED: "Desativado",
    ARCHIVED: "Arquivado",
  };
  return labels[value] ?? value;
}
