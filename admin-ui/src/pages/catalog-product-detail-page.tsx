import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getItem,
  getPriceVersion,
  getProduct,
  listAllCatalogEntries,
  publishPriceVersion,
  updateItem,
  updateProduct,
  type ItemResponse,
  type PriceVersionResponse,
} from "@/api/catalog-api";
import { QueryEmpty, QueryError, QueryLoading } from "@/components/query-feedback";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";
import { formatDate, formatUnits } from "@/lib/format";

interface ProductCatalogGraph {
  product: Awaited<ReturnType<typeof getProduct>>;
  items: ItemResponse[];
  prices: PriceVersionResponse[];
}

export function CatalogProductDetailPage() {
  const { id } = useParams();
  const queryClient = useQueryClient();
  const [priceToPublish, setPriceToPublish] = useState<PriceVersionResponse | null>(null);
  const [confirmActivation, setConfirmActivation] = useState(false);
  const graph = useQuery({
    queryKey: ["catalog-product-graph", id],
    enabled: Boolean(id),
    queryFn: () => loadProductGraph(id!),
  });
  const publish = useMutation({
    mutationFn: (priceId: string) => publishPriceVersion(priceId),
    onSuccess: () => refreshGraph(queryClient, id),
    onError: () => refreshGraph(queryClient, id),
  });
  const activate = useMutation({
    mutationFn: (current: ProductCatalogGraph) => activateProductGraph(current),
    onSuccess: () => refreshGraph(queryClient, id),
    onError: () => refreshGraph(queryClient, id),
  });
  if (!id) return <p>Produto desconhecido.</p>;
  const current = graph.data;
  const publishable = current && canActivateProduct(current);
  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-6">
      <header className="flex flex-col gap-3">
        <Link className="inline-flex items-center gap-2 text-sm text-primary hover:underline" to="/catalog/products">
          <ArrowLeft /> Produtos
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">{current?.product.name ?? "Detalhe do produto"}</h1>
        <p className="break-all font-mono text-sm text-muted-foreground">{id}</p>
      </header>
      {graph.isLoading && <QueryLoading />}
      {graph.error && <QueryError error={graph.error} />}
      {publish.error && <QueryError error={publish.error} />}
      {activate.error && <QueryError error={activate.error} />}
      {current && (
        <>
          <Card>
            <CardHeader>
              <CardTitle className="flex flex-wrap items-center gap-3">
                {current.product.name}
                <Badge variant={current.product.status === "ACTIVE" ? "default" : "outline"}>{current.product.status}</Badge>
              </CardTitle>
              <CardDescription>{current.product.description || "Sem descrição."}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <Table>
                <TableBody>
                  <TableRow><TableCell className="text-muted-foreground">Modelo</TableCell><TableCell>{current.product.usage_model === "CREDIT_METERED" ? "Consumo cobrado em créditos" : "Acesso por assinatura"}</TableCell></TableRow>
                  <TableRow><TableCell className="text-muted-foreground">Criado</TableCell><TableCell>{formatDate(current.product.created_at)}</TableCell></TableRow>
                  <TableRow><TableCell className="text-muted-foreground">Itens</TableCell><TableCell>{current.items.length}</TableCell></TableRow>
                </TableBody>
              </Table>
              {current.product.usage_model === "CREDIT_METERED" && current.product.status !== "ACTIVE" && (
                <Button variant="outline" nativeButton={false} render={<Link to={`/catalog/items/new?product_id=${id}`} />}>
                  Adicionar item de consumo
                </Button>
              )}
              {current.product.usage_model === "CREDIT_METERED" && current.product.status !== "ACTIVE" && (
                <Button disabled={!publishable || activate.isPending} onClick={() => setConfirmActivation(true)}>
                  {activate.isPending ? "Ativando…" : "Ativar produto"}
                </Button>
              )}
              {current.product.usage_model === "ENTITLEMENT_ONLY" && (
                <p className="text-sm text-muted-foreground">A publicação deste modelo ainda não está disponível.</p>
              )}
              {current.product.usage_model === "CREDIT_METERED" && current.product.status !== "ACTIVE" && !publishable && (
                <p className="text-sm text-muted-foreground">Publique ao menos um preço de cada item antes de ativar o produto.</p>
              )}
            </CardContent>
          </Card>
          {current.items.length === 0 && <QueryEmpty title="Nenhum item" description="Este produto não tem itens de consumo cadastrados." />}
          {current.items.map((item) => (
            <Card key={item.item_id}>
              <CardHeader>
                <CardTitle className="flex flex-wrap items-center gap-3">
                  {item.name}<Badge variant="outline">{item.status}</Badge>
                </CardTitle>
                <CardDescription>
                  {item.unit_name ? `${item.unit_name} · escala ${item.quantity_scale}` : "Escopo de acesso"}
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-3">
                {current.prices.filter((price) => price.item_id === item.item_id).map((price) => (
                  <div key={price.price_version_id} className="flex flex-wrap items-center justify-between gap-3 rounded-md border p-3">
                    <div className="text-sm">
                      <p>{describePrice(price)}</p>
                      <p className="text-muted-foreground">Vigência: {formatDate(price.effective_from)} até {formatDate(price.effective_until)}</p>
                    </div>
                    <Badge variant={price.state === "DRAFT" ? "outline" : "secondary"}>{price.state}</Badge>
                    {price.state === "DRAFT" && (
                      <Button size="sm" disabled={publish.isPending} onClick={() => setPriceToPublish(price)}>
                        {publish.isPending && publish.variables === price.price_version_id ? "Publicando…" : "Publicar preço"}
                      </Button>
                    )}
                  </div>
                ))}
                {current.prices.every((price) => price.item_id !== item.item_id) && (
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <p className="text-sm text-muted-foreground">Este item ainda não tem preço.</p>
                    <Button size="sm" variant="outline" nativeButton={false} render={<Link to={`/catalog/prices/new?item_id=${item.item_id}`} />}>Adicionar preço</Button>
                  </div>
                )}
              </CardContent>
            </Card>
          ))}
        </>
      )}
      <AlertDialog open={Boolean(priceToPublish)} onOpenChange={(open) => !open && setPriceToPublish(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Publicar este preço?</AlertDialogTitle>
            <AlertDialogDescription>
              {priceToPublish && `${describePrice(priceToPublish)}. Depois da publicação, esta versão não pode ser editada.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Voltar</AlertDialogCancel>
            <AlertDialogAction disabled={publish.isPending} onClick={() => {
              if (priceToPublish) publish.mutate(priceToPublish.price_version_id);
              setPriceToPublish(null);
            }}>Confirmar publicação</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog open={confirmActivation} onOpenChange={setConfirmActivation}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Ativar produto e itens?</AlertDialogTitle>
            <AlertDialogDescription>
              {current && `${current.items.length} item(ns) serão ativados antes do produto. Isso publica a configuração de catálogo; não cria assinaturas nem concede acesso a clientes.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Voltar</AlertDialogCancel>
            <AlertDialogAction disabled={activate.isPending} onClick={() => {
              if (current) activate.mutate(current);
              setConfirmActivation(false);
            }}>Confirmar ativação</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

async function loadProductGraph(productId: string): Promise<ProductCatalogGraph> {
  const [product, entries] = await Promise.all([
    getProduct(productId),
    listAllCatalogEntries("items", productId),
  ]);
  const items = await Promise.all(entries.map((entry) => getItem(entry.id)));
  const priceEntries = await Promise.all(items.map((item) => listAllCatalogEntries("prices", item.item_id)));
  const prices = await Promise.all(priceEntries.flat().map((entry) => getPriceVersion(entry.id)));
  return { product, items, prices };
}

function canActivateProduct(graph: ProductCatalogGraph): boolean {
  return graph.items.length > 0 && graph.items.every((item) =>
    graph.prices.some((price) => price.item_id === item.item_id && ["ACTIVE", "SCHEDULED"].includes(price.state)),
  );
}

async function activateProductGraph(graph: ProductCatalogGraph): Promise<void> {
  for (const item of graph.items) {
    if (item.status === "ACTIVE") continue;
    await updateItem(item.item_id, { status: "ACTIVE", expected_version: item.version });
  }
  if (graph.product.status !== "ACTIVE") {
    await updateProduct(graph.product.product_id, {
      status: "ACTIVE",
      expected_version: graph.product.version,
    });
  }
}

async function refreshGraph(queryClient: ReturnType<typeof useQueryClient>, id?: string) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["catalog-product-graph", id] }),
    queryClient.invalidateQueries({ queryKey: ["catalog", "products"] }),
    queryClient.invalidateQueries({ queryKey: ["catalog", "items"] }),
    queryClient.invalidateQueries({ queryKey: ["catalog", "prices"] }),
  ]);
}

function describePrice(price: PriceVersionResponse): string {
  if (price.pricing_model === "unit") {
    return `A cada ${formatUnits(price.unit_block_size)} unidade(s), cobrar ${formatUnits(price.credit_units)} créditos`;
  }
  return `${price.tiers.length} faixa(s) de consumo acumulado`;
}
