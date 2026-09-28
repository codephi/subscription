import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Pencil } from "lucide-react";
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
  type ProductResponse,
} from "@/api/catalog-api";
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
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { formatDate, formatUnits } from "@/lib/format";
import { Input } from "@/components/ui/input";
import {
  CatalogItemCard,
  type CatalogItemEditRequest,
} from "@/components/catalog-item-card";

interface ProductCatalogGraph {
  product: ProductResponse;
  items: ItemResponse[];
  prices: PriceVersionResponse[];
}

interface ProductEditDraft {
  name: string;
  description: string;
  usageModel: ProductResponse["usage_model"];
  status: ProductResponse["status"];
}

export function CatalogProductDetailPage() {
  const { id } = useParams();
  const queryClient = useQueryClient();
  const [priceToPublish, setPriceToPublish] =
    useState<PriceVersionResponse | null>(null);
  const [confirmActivation, setConfirmActivation] = useState(false);
  const [editing, setEditing] = useState(false);
  const [productDraft, setProductDraft] = useState<ProductEditDraft | null>(
    null,
  );
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
  const saveProduct = useMutation({
    mutationFn: (draft: ProductEditDraft & { expected_version: number }) =>
      updateProduct(id!, {
        name: draft.name,
        description: draft.description.trim() || null,
        usage_model: draft.usageModel,
        status: draft.status,
        expected_version: draft.expected_version,
      }),
    onSuccess: () => {
      setEditing(false);
      setProductDraft(null);
      void refreshGraph(queryClient, id);
      void queryClient.invalidateQueries({
        queryKey: ["catalog-detail", "products", id],
      });
    },
    onError: () => refreshGraph(queryClient, id),
  });
  const saveItem = useMutation({
    mutationFn: ({
      itemId,
      request,
    }: {
      itemId: string;
      request: CatalogItemEditRequest;
    }) => updateItem(itemId, request),
    onSuccess: () => refreshGraph(queryClient, id),
    onError: () => refreshGraph(queryClient, id),
  });
  if (!id) return <p>Produto desconhecido.</p>;
  const current = graph.data;
  const publishable = current && canActivateProduct(current);
  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-6">
      <header className="flex flex-col gap-3">
        <Link
          className="inline-flex items-center gap-2 text-sm text-primary hover:underline"
          to="/catalog/products"
        >
          <ArrowLeft /> Produtos
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">
          {current?.product.name ?? "Detalhe do produto"}
        </h1>
        <p className="break-all font-mono text-sm text-muted-foreground">
          {id}
        </p>
      </header>
      {graph.isLoading && <QueryLoading />}
      {graph.error && <QueryError error={graph.error} />}
      {publish.error && <QueryError error={publish.error} />}
      {activate.error && <QueryError error={activate.error} />}
      {saveProduct.error && <QueryError error={saveProduct.error} />}
      {saveItem.error && <QueryError error={saveItem.error} />}
      {current && (
        <>
          {editing && productDraft ? (
            <Card>
              <CardHeader>
                <CardTitle>Editar produto</CardTitle>
                <CardDescription>
                  Altere os dados e a disponibilidade do produto.
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <label
                  className="flex flex-col gap-2 text-sm font-medium"
                  htmlFor="edit-product-name"
                >
                  Nome
                  <Input
                    id="edit-product-name"
                    value={productDraft.name}
                    onChange={(event) =>
                      setProductDraft({
                        ...productDraft,
                        name: event.target.value,
                      })
                    }
                    required
                  />
                </label>
                <label
                  className="flex flex-col gap-2 text-sm font-medium"
                  htmlFor="edit-product-description"
                >
                  Descrição
                  <Textarea
                    id="edit-product-description"
                    value={productDraft.description}
                    onChange={(event) =>
                      setProductDraft({
                        ...productDraft,
                        description: event.target.value,
                      })
                    }
                    placeholder="Sem descrição"
                  />
                </label>
                <label
                  className="flex flex-col gap-2 text-sm font-medium"
                  htmlFor="edit-product-model"
                >
                  Modelo de produto
                  <select
                    id="edit-product-model"
                    className="h-9 rounded-md border bg-transparent px-3 text-sm"
                    value={productDraft.usageModel}
                    onChange={(event) => {
                      const usageModel = event.target
                        .value as ProductResponse["usage_model"];
                      setProductDraft({
                        ...productDraft,
                        usageModel,
                        status:
                          usageModel === "ENTITLEMENT_ONLY" &&
                          productDraft.status === "ACTIVE"
                            ? "INACTIVE"
                            : productDraft.status,
                      });
                    }}
                  >
                    <option value="CREDIT_METERED">
                      Consumo cobrado em créditos
                    </option>
                    <option value="ENTITLEMENT_ONLY">
                      Acesso por assinatura
                    </option>
                  </select>
                </label>
                <label
                  className="flex flex-col gap-2 text-sm font-medium"
                  htmlFor="edit-product-status"
                >
                  Status
                  <select
                    id="edit-product-status"
                    className="h-9 rounded-md border bg-transparent px-3 text-sm"
                    value={productDraft.status}
                    onChange={(event) =>
                      setProductDraft({
                        ...productDraft,
                        status: event.target.value as ProductResponse["status"],
                      })
                    }
                  >
                    <option value="INACTIVE">Inativo</option>
                    <option
                      value="ACTIVE"
                      disabled={productDraft.usageModel === "ENTITLEMENT_ONLY"}
                    >
                      Ativo
                    </option>
                    <option value="ARCHIVED">Arquivado</option>
                  </select>
                </label>
                <div className="flex justify-end gap-2">
                  <Button variant="outline" onClick={() => setEditing(false)}>
                    Cancelar
                  </Button>
                  <Button
                    disabled={
                      saveProduct.isPending || !productDraft.name.trim()
                    }
                    onClick={() =>
                      saveProduct.mutate({
                        ...productDraft,
                        expected_version: current.product.version,
                      })
                    }
                  >
                    {saveProduct.isPending ? "Salvando…" : "Salvar alterações"}
                  </Button>
                </div>
              </CardContent>
            </Card>
          ) : (
            <Card>
              <CardHeader>
                <CardTitle className="flex flex-wrap items-center gap-3">
                  {current.product.name}
                  <Badge
                    variant={
                      current.product.status === "ACTIVE"
                        ? "default"
                        : "outline"
                    }
                  >
                    {current.product.status}
                  </Badge>
                </CardTitle>
                <CardDescription>
                  {current.product.description || "Sem descrição."}
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <Button
                  variant="outline"
                  className="self-start"
                  onClick={() => {
                    setProductDraft({
                      name: current.product.name,
                      description: current.product.description ?? "",
                      usageModel: current.product.usage_model,
                      status: current.product.status,
                    });
                    setEditing(true);
                  }}
                >
                  <Pencil data-icon="inline-start" /> Editar produto
                </Button>
                <Table>
                  <TableBody>
                    <TableRow>
                      <TableCell className="text-muted-foreground">
                        Modelo
                      </TableCell>
                      <TableCell>
                        {current.product.usage_model === "CREDIT_METERED"
                          ? "Consumo cobrado em créditos"
                          : "Acesso por assinatura"}
                      </TableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell className="text-muted-foreground">
                        Criado
                      </TableCell>
                      <TableCell>
                        {formatDate(current.product.created_at)}
                      </TableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell className="text-muted-foreground">
                        Itens
                      </TableCell>
                      <TableCell>{current.items.length}</TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
                {current.product.usage_model === "CREDIT_METERED" &&
                  current.product.status !== "ACTIVE" && (
                    <Button
                      variant="outline"
                      nativeButton={false}
                      render={
                        <Link to={`/catalog/items/new?product_id=${id}`} />
                      }
                    >
                      Adicionar item de consumo
                    </Button>
                  )}
                {current.product.usage_model === "CREDIT_METERED" &&
                  current.product.status !== "ACTIVE" && (
                    <Button
                      disabled={!publishable || activate.isPending}
                      onClick={() => setConfirmActivation(true)}
                    >
                      {activate.isPending ? "Ativando…" : "Ativar produto"}
                    </Button>
                  )}
                {current.product.usage_model === "ENTITLEMENT_ONLY" && (
                  <p className="text-sm text-muted-foreground">
                    A publicação deste modelo ainda não está disponível.
                  </p>
                )}
                {current.product.usage_model === "CREDIT_METERED" &&
                  current.product.status !== "ACTIVE" &&
                  !publishable && (
                    <p className="text-sm text-muted-foreground">
                      Publique ao menos um preço de cada item antes de ativar o
                      produto.
                    </p>
                  )}
              </CardContent>
            </Card>
          )}
          {current.items.length === 0 && (
            <QueryEmpty
              title="Nenhum item"
              description="Este produto não tem itens de consumo cadastrados."
            />
          )}
          {current.items.map((item) => (
            <CatalogItemCard
              key={item.item_id}
              item={item}
              items={current.items}
              prices={current.prices}
              usageModel={current.product.usage_model}
              saving={
                saveItem.isPending &&
                saveItem.variables?.itemId === item.item_id
              }
              publishingPriceId={
                publish.isPending ? (publish.variables ?? null) : null
              }
              onSave={(request) =>
                saveItem.mutateAsync({ itemId: item.item_id, request })
              }
              onPublishPrice={setPriceToPublish}
            />
          ))}
        </>
      )}
      <AlertDialog
        open={Boolean(priceToPublish)}
        onOpenChange={(open) => !open && setPriceToPublish(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Publicar este preço?</AlertDialogTitle>
            <AlertDialogDescription>
              {priceToPublish &&
                `${describePrice(priceToPublish)}. Depois da publicação, esta versão não pode ser editada.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Voltar</AlertDialogCancel>
            <AlertDialogAction
              disabled={publish.isPending}
              onClick={() => {
                if (priceToPublish)
                  publish.mutate(priceToPublish.price_version_id);
                setPriceToPublish(null);
              }}
            >
              Confirmar publicação
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog open={confirmActivation} onOpenChange={setConfirmActivation}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Ativar produto e itens?</AlertDialogTitle>
            <AlertDialogDescription>
              {current &&
                `${current.items.length} item(ns) serão ativados antes do produto. Isso publica a configuração de catálogo; não cria assinaturas nem concede acesso a clientes.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Voltar</AlertDialogCancel>
            <AlertDialogAction
              disabled={activate.isPending}
              onClick={() => {
                if (current) activate.mutate(current);
                setConfirmActivation(false);
              }}
            >
              Confirmar ativação
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

async function loadProductGraph(
  productId: string,
): Promise<ProductCatalogGraph> {
  const [product, entries] = await Promise.all([
    getProduct(productId),
    listAllCatalogEntries("items", productId),
  ]);
  const items = await Promise.all(entries.map((entry) => getItem(entry.id)));
  const priceEntries = await Promise.all(
    items.map((item) => listAllCatalogEntries("prices", item.item_id)),
  );
  const prices = await Promise.all(
    priceEntries.flat().map((entry) => getPriceVersion(entry.id)),
  );
  return { product, items, prices };
}

function canActivateProduct(graph: ProductCatalogGraph): boolean {
  return (
    graph.items.length > 0 &&
    graph.items.every((item) =>
      graph.prices.some(
        (price) =>
          price.item_id === item.item_id &&
          ["ACTIVE", "SCHEDULED"].includes(price.state),
      ),
    )
  );
}

async function activateProductGraph(graph: ProductCatalogGraph): Promise<void> {
  for (const item of graph.items) {
    if (item.status === "ACTIVE") continue;
    await updateItem(item.item_id, {
      status: "ACTIVE",
      expected_version: item.version,
    });
  }
  if (graph.product.status !== "ACTIVE") {
    await updateProduct(graph.product.product_id, {
      status: "ACTIVE",
      expected_version: graph.product.version,
    });
  }
}

async function refreshGraph(
  queryClient: ReturnType<typeof useQueryClient>,
  id?: string,
) {
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
