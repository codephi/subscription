import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ArrowLeft } from "lucide-react";
import { Link, useParams } from "react-router-dom";
import {
  getCatalogDetail,
  publishPriceVersion,
  revokeSubscriptionPlan,
} from "@/api/catalog-api";
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
import { QueryError, QueryLoading } from "@/components/query-feedback";
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
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { catalogKinds, parseCatalogKind } from "@/lib/catalog-kinds";

export function CatalogDetailPage() {
  const { kind: rawKind, id } = useParams();
  const kind = parseCatalogKind(rawKind);
  const queryClient = useQueryClient();
  const [revokeReason, setRevokeReason] = useState("");
  const [actorReference, setActorReference] = useState("");
  const detail = useQuery({
    queryKey: ["catalog-detail", kind, id],
    queryFn: () => getCatalogDetail(kind!, id!),
    enabled: !!kind && !!id,
  });
  const publish = useMutation({
    mutationFn: () => publishPriceVersion(id!),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["catalog-detail", "prices", id],
      });
      await queryClient.invalidateQueries({ queryKey: ["catalog", "prices"] });
    },
  });
  const revoke = useMutation({
    mutationFn: () =>
      revokeSubscriptionPlan(id!, {
        reason: revokeReason,
        actor_reference: actorReference,
      }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["catalog-detail", "plans", id],
      });
      await queryClient.invalidateQueries({ queryKey: ["catalog", "plans"] });
    },
  });
  if (!kind || !id) return <p>Registro de catálogo desconhecido.</p>;
  const fields = detail.data ? Object.entries(detail.data) : [];
  const immutable =
    ["plans", "on-demand", "policies"].includes(kind) ||
    (kind === "prices" &&
      fields.some(([key, value]) => key === "state" && value !== "DRAFT"));
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-3">
        <Link
          className="inline-flex items-center gap-2 text-sm text-primary hover:underline"
          to={`/catalog/${kind}`}
        >
          <ArrowLeft className="size-4" />
          {catalogKinds[kind]}
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">
          Detalhe da oferta
        </h1>
        <p className="break-all font-mono text-sm text-muted-foreground">
          {id}
        </p>
      </header>
      {detail.isLoading && <QueryLoading />}
      {detail.error && <QueryError error={detail.error} />}
      {publish.error && <QueryError error={publish.error} />}
      {revoke.error && <QueryError error={revoke.error} />}
      {detail.data && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-3">
              {catalogKinds[kind]}{" "}
              {immutable && <Badge variant="secondary">Versão imutável</Badge>}
            </CardTitle>
            <CardDescription>Valores retornados pela API.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <Table>
              <TableBody>
                {fields.map(([label, value]) => (
                  <TableRow key={label}>
                    <TableCell className="w-56 text-muted-foreground">
                      {label}
                    </TableCell>
                    <TableCell className="break-all font-mono text-xs">
                      {value === null
                        ? "—"
                        : typeof value === "object"
                          ? JSON.stringify(value)
                          : String(value)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {kind === "prices" &&
              Object.entries(detail.data).some(
                ([key, value]) => key === "state" && value === "DRAFT",
              ) && (
                <AlertDialog>
                  <AlertDialogTrigger render={<Button variant="outline" />}>
                    Publicar versão
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Publicar este preço?</AlertDialogTitle>
                      <AlertDialogDescription>
                        Confirme a vigência, os blocos e os créditos acima.
                        Depois da publicação, esta versão não pode ser editada.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel>Voltar</AlertDialogCancel>
                      <AlertDialogAction
                        disabled={publish.isPending}
                        onClick={() => publish.mutate()}
                      >
                        Publicar
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}
            {kind === "plans" &&
              Object.entries(detail.data).some(
                ([key, value]) => key === "revoked_at" && value === null,
              ) && (
                <AlertDialog>
                  <AlertDialogTrigger render={<Button variant="destructive" />}>
                    Revogar versão
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Revogar este plano?</AlertDialogTitle>
                      <AlertDialogDescription>
                        O plano deixará de poder ser usado em novas adesões. Os
                        clientes existentes mantêm seus registros comerciais.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <FieldGroup>
                      <Field>
                        <FieldLabel htmlFor="plan-revoke-reason">
                          Motivo
                        </FieldLabel>
                        <Input
                          id="plan-revoke-reason"
                          value={revokeReason}
                          onChange={(event) =>
                            setRevokeReason(event.target.value)
                          }
                          required
                        />
                      </Field>
                      <Field>
                        <FieldLabel htmlFor="plan-revoke-actor">
                          Referência operacional
                        </FieldLabel>
                        <Input
                          id="plan-revoke-actor"
                          value={actorReference}
                          onChange={(event) =>
                            setActorReference(event.target.value)
                          }
                          required
                        />
                      </Field>
                    </FieldGroup>
                    <AlertDialogFooter>
                      <AlertDialogCancel>Voltar</AlertDialogCancel>
                      <AlertDialogAction
                        disabled={
                          revoke.isPending ||
                          !revokeReason.trim() ||
                          !actorReference.trim()
                        }
                        onClick={() => revoke.mutate()}
                      >
                        Confirmar revogação
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
