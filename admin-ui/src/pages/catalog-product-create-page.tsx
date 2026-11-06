import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { AlertTriangle, Plus } from "lucide-react";
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
import { QueryError } from "@/components/query-feedback";
import { ProductItemCard } from "@/components/catalog-product-item-fields";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
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
  newCatalogItemDraft,
  newCatalogProductDraft,
  validateCatalogProductDraft,
  type CatalogProductDraft,
  type CatalogProductItemDraft,
} from "@/lib/catalog-product";
import {
  canResumeProductCreation,
  productWorkflowClient,
  runProductCreation,
  type ProductCreationProgress,
} from "@/lib/catalog-product-workflow";

const SESSION_KEY = "catalog-product-creation-v1";

export function CatalogProductCreatePage() {
  const navigate = useNavigate();
  const [progress, setProgress] = useState<ProductCreationProgress | null>(
    readProgress,
  );
  const [draft, setDraft] = useState(
    progress?.draft ?? newCatalogProductDraft(),
  );
  const [pending, setPending] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [validationError, setValidationError] = useState<string | null>(null);
  const locked = Boolean(progress?.productId || progress?.uncertain);
  const completeDraft = withSuggestedItemNames(draft);

  function updateDraft(next: CatalogProductDraft) {
    setDraft(next);
    if (progress && !progress.productId)
      saveProgress({ ...progress, draft: next });
  }

  function persistProgress(next: ProductCreationProgress) {
    saveProgress(next);
    setProgress(next);
  }

  function updateItem(
    itemId: string,
    update: Partial<CatalogProductItemDraft>,
  ) {
    updateDraft({
      ...draft,
      items: draft.items.map((item) =>
        item.draftId === itemId ? { ...item, ...update } : item,
      ),
    });
  }

  function start(publish: boolean) {
    const issue = validateCatalogProductDraft(completeDraft, publish);
    if (issue) {
      setValidationError(issue);
      return;
    }
    setValidationError(null);
    setError(null);
    if (publish) {
      setReviewOpen(true);
      return;
    }
    void submit(completeDraft, false);
  }

  async function submit(selectedDraft: CatalogProductDraft, publish: boolean) {
    setPending(true);
    const initial = progress ?? {
      draft: selectedDraft,
      productId: null,
      itemIds: {},
      priceIds: {},
      step: null,
      uncertain: false,
      publishing: publish,
    };
    if (!progress) persistProgress(initial);
    try {
      const productId = await runProductCreation(
        { ...initial, draft: selectedDraft, publishing: publish },
        publish,
        productWorkflowClient,
        persistProgress,
      );
      sessionStorage.removeItem(SESSION_KEY);
      navigate(`/catalog/products/${productId}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason : new Error(String(reason)));
      const saved = readProgress();
      if (saved) {
        setProgress(saved);
        setDraft(saved.draft);
      }
    } finally {
      setPending(false);
      setReviewOpen(false);
    }
  }

  function releaseUncertainAttempt() {
    sessionStorage.removeItem(SESSION_KEY);
    setProgress(null);
    setDraft(newCatalogProductDraft());
    setError(null);
    setValidationError(null);
  }

  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link
          className="text-sm text-primary hover:underline"
          to="/catalog/products"
        >
          ← Produtos
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">Criar produto</h1>
        <p className="text-muted-foreground">
          Cadastre o produto e configure como o consumo vira créditos.
        </p>
      </header>

      {progress && (
        <Card>
          <CardHeader>
            <CardTitle>
              {progress.uncertain
                ? "Confira o catálogo"
                : "Cadastro em andamento"}
            </CardTitle>
            <CardDescription>
              {progress.uncertain
                ? "A última chamada não confirmou o resultado. Não repetimos a etapa automaticamente."
                : "As etapas concluídas foram guardadas nesta sessão. Continue para terminar."}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-3">
            {progress.productId && (
              <Button
                nativeButton={false}
                render={<Link to={`/catalog/products/${progress.productId}`} />}
              >
                Conferir produto criado
              </Button>
            )}
            {progress.uncertain && !canResumeProductCreation(progress) ? (
              <>
                <Button
                  variant="outline"
                  nativeButton={false}
                  render={<Link to="/catalog/products" />}
                >
                  Abrir lista de produtos
                </Button>
                <Button variant="destructive" onClick={releaseUncertainAttempt}>
                  Já conferi. Começar outro cadastro
                </Button>
              </>
            ) : (
              <Button
                disabled={pending}
                onClick={() => void submit(progress.draft, progress.publishing)}
              >
                {pending
                  ? "Conferindo e continuando…"
                  : progress.uncertain
                    ? "Conferir resultado e continuar"
                    : "Continuar cadastro"}
              </Button>
            )}
          </CardContent>
        </Card>
      )}

      <form
        className="flex flex-col gap-5"
        onSubmit={(event) => event.preventDefault()}
      >
        <Card>
          <CardHeader>
            <CardTitle>Produto</CardTitle>
            <CardDescription>
              Comece pelo nome; depois informe a unidade e o custo do consumo.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <FieldGroup className="grid gap-4 md:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="product-name">Nome</FieldLabel>
                <Input
                  id="product-name"
                  value={draft.name}
                  onChange={(event) =>
                    updateDraft({ ...draft, name: event.target.value })
                  }
                  placeholder="Ex.: API de inteligência"
                  disabled={locked}
                  required
                />
              </Field>
              <Field className="md:col-span-2">
                <details>
                  <summary className="cursor-pointer text-sm font-medium">
                    Adicionar descrição
                  </summary>
                  <Input
                    id="product-description"
                    className="mt-3"
                    value={draft.description}
                    onChange={(event) =>
                      updateDraft({ ...draft, description: event.target.value })
                    }
                    disabled={locked}
                    placeholder="Descreva este produto"
                  />
                </details>
              </Field>
              <Field className="md:col-span-2">
                <details>
                  <summary className="cursor-pointer text-sm font-medium">
                    Configurações avançadas
                  </summary>
                  <div className="mt-4 flex flex-col gap-2">
                    <FieldLabel htmlFor="product-usage-model">
                      Modelo de produto
                    </FieldLabel>
                    <Select
                      items={[
                        {
                          label: "Consumo cobrado em créditos",
                          value: "CREDIT_METERED",
                        },
                        {
                          label:
                            "Acesso por assinatura, sem cobrança por consumo",
                          value: "ENTITLEMENT_ONLY",
                        },
                      ]}
                      value={draft.usageModel}
                      onValueChange={(value) =>
                        updateDraft({
                          ...draft,
                          usageModel:
                            value === "ENTITLEMENT_ONLY"
                              ? value
                              : "CREDIT_METERED",
                          items: draft.items.length
                            ? draft.items
                            : [newCatalogItemDraft(draft.name)],
                        })
                      }
                      disabled={locked}
                    >
                      <SelectTrigger
                        id="product-usage-model"
                        className="w-full"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          <SelectItem value="CREDIT_METERED">
                            Consumo cobrado em créditos
                          </SelectItem>
                          <SelectItem value="ENTITLEMENT_ONLY">
                            Acesso por assinatura, sem cobrança por consumo
                          </SelectItem>
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      Acesso por assinatura pode ser salvo como inativo; a
                      publicação desse modelo ainda não está disponível.
                    </FieldDescription>
                  </div>
                </details>
              </Field>
            </FieldGroup>
          </CardContent>
        </Card>

        {(draft.usageModel === "CREDIT_METERED" || draft.items.length > 0) && (
          <Card>
            <CardHeader>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex flex-col gap-1">
                  <CardTitle>Consumo e preço</CardTitle>
                  <CardDescription>
                    Sem IDs: os itens e preços serão ligados ao produto
                    automaticamente.
                  </CardDescription>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  disabled={locked}
                  onClick={() =>
                    updateDraft({
                      ...draft,
                      items: [...draft.items, newCatalogItemDraft()],
                    })
                  }
                >
                  <Plus data-icon="inline-start" /> Adicionar item
                </Button>
              </div>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              {draft.usageModel === "ENTITLEMENT_ONLY" ? (
                <p className="rounded-md border p-4 text-sm text-muted-foreground">
                  Acesso por assinatura não possui preço de consumo. Os valores
                  preenchidos ficam guardados caso você volte para cobrança por
                  créditos.
                </p>
              ) : (
                draft.items.map((item, index) => (
                  <ProductItemCard
                    key={item.draftId}
                    index={index}
                    item={item}
                    items={draft.items}
                    suggestedName={draft.name}
                    locked={locked}
                    onChange={(update) => updateItem(item.draftId, update)}
                    onRemove={() =>
                      updateDraft({
                        ...draft,
                        items: draft.items.filter(
                          (candidate) => candidate.draftId !== item.draftId,
                        ),
                      })
                    }
                  />
                ))
              )}
              {draft.items.length === 0 && (
                <p className="text-sm text-muted-foreground">
                  Você pode salvar somente o produto e configurar o consumo
                  depois.
                </p>
              )}
            </CardContent>
          </Card>
        )}

        <Card>
          <CardHeader>
            <CardTitle>Resumo</CardTitle>
            <CardDescription>
              {draft.usageModel === "ENTITLEMENT_ONLY"
                ? "Acesso por assinatura será salvo inativo."
                : draft.items.length
                  ? `${draft.items.length} item(ns) de consumo · preços ficam em rascunho até publicar.`
                  : "Somente o produto será salvo como inativo."}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            {draft.usageModel === "CREDIT_METERED" &&
              completeDraft.items.map((item) => (
                <p key={item.draftId} className="text-sm">
                  <strong>{item.name || "Novo item"}</strong>:{" "}
                  {describeDraftPrice(item)}
                </p>
              ))}
            <p className="text-sm text-muted-foreground">
              Publicar o catálogo não cria assinaturas nem concede acesso a
              clientes. Preços publicados são imutáveis.
            </p>
          </CardContent>
        </Card>

        {validationError && (
          <p role="alert" className="text-sm text-destructive">
            {validationError}
          </p>
        )}
        {error && <QueryError error={error} />}
        {progress?.uncertain && (
          <p
            role="alert"
            className="flex items-center gap-2 text-sm text-destructive"
          >
            <AlertTriangle data-icon="inline-start" /> Confira a etapa em
            andamento antes de liberar uma nova tentativa.
          </p>
        )}
        {!locked && (
          <div className="flex flex-wrap justify-end gap-3">
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={() => start(false)}
            >
              {pending ? "Salvando…" : "Salvar sem publicar"}
            </Button>
            <Button
              type="button"
              disabled={pending || draft.usageModel === "ENTITLEMENT_ONLY"}
              onClick={() => start(true)}
            >
              Revisar e publicar
            </Button>
            {draft.usageModel === "ENTITLEMENT_ONLY" && (
              <p className="w-full text-right text-sm text-muted-foreground">
                Acesso por assinatura só pode ser salvo inativo.
              </p>
            )}
          </div>
        )}
      </form>

      <AlertDialog open={reviewOpen} onOpenChange={setReviewOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Revisar e publicar “{draft.name.trim()}”?
            </AlertDialogTitle>
            <AlertDialogDescription>
              {completeDraft.items.map((item) => (
                <span key={item.draftId} className="block">
                  {item.name}: {describeDraftPrice(item)}{" "}
                  {describeDraftValidity(item)}
                </span>
              ))}
              Preços publicados são imutáveis. Isso não cria assinatura nem
              concede acesso a clientes.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Voltar</AlertDialogCancel>
            <AlertDialogAction
              disabled={pending}
              onClick={() => void submit(completeDraft, true)}
            >
              {pending ? "Publicando…" : "Confirmar publicação"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function withSuggestedItemNames(
  draft: CatalogProductDraft,
): CatalogProductDraft {
  return {
    ...draft,
    items: draft.items.map((item) => ({
      ...item,
      name: item.name.trim() || draft.name.trim(),
    })),
  };
}

function displayUnits(value: string): string {
  if (!/^\d+$/.test(value)) return "—";
  return BigInt(value).toLocaleString("pt-BR");
}

function describeDraftPrice(item: CatalogProductItemDraft): string {
  if (item.pricingModel === "unit") {
    return `a cada ${displayUnits(item.unitBlockSize)} ${item.unitName || "unidade"} consumida(s), cobrar ${displayUnits(item.creditUnits)} créditos.`;
  }
  return `preço por faixas: ${item.tiers
    .map((tier) => {
      const range = tier.to
        ? `${displayUnits(tier.from)}–${displayUnits(tier.to)}`
        : `a partir de ${displayUnits(tier.from)}`;
      return `${range} unidades, bloco de ${displayUnits(tier.block)} por ${displayUnits(tier.credits)} créditos`;
    })
    .join("; ")}.`;
}

function describeDraftValidity(item: CatalogProductItemDraft): string {
  const start = item.effectiveFrom
    ? new Date(item.effectiveFrom).toLocaleString("pt-BR")
    : "o envio";
  const end = item.effectiveUntil
    ? new Date(item.effectiveUntil).toLocaleString("pt-BR")
    : "sem término";
  return `Vigência de ${start} a ${end}.`;
}

function readProgress(): ProductCreationProgress | null {
  try {
    const raw = sessionStorage.getItem(SESSION_KEY);
    return raw ? (JSON.parse(raw) as ProductCreationProgress) : null;
  } catch {
    return null;
  }
}

function saveProgress(progress: ProductCreationProgress) {
  sessionStorage.setItem(SESSION_KEY, JSON.stringify(progress));
}
