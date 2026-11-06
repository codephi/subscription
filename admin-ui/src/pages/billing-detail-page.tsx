import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import { Link, useParams } from "react-router-dom";
import { getBillingRecord, replayOutbox } from "@/api/billing-api";
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
import { billingKinds, parseBillingKind } from "@/lib/billing-records";
import { formatDate } from "@/lib/format";

export function BillingDetailPage() {
  const { kind: rawKind, id } = useParams();
  const kind = parseBillingKind(rawKind);
  const client = useQueryClient();
  const record = useQuery({
    queryKey: ["billing-record", kind, id],
    queryFn: () => getBillingRecord(kind!, id!),
    enabled: !!kind && !!id,
  });
  const replay = useMutation({
    mutationFn: () => replayOutbox(id!),
    onSuccess: async () => {
      await client.invalidateQueries({
        queryKey: ["billing-record", "outbox", id],
      });
      await client.invalidateQueries({
        queryKey: ["billing-records", "outbox"],
      });
      await client.invalidateQueries({ queryKey: ["operations"] });
    },
  });
  if (!kind || !id) return <p>Registro de Billing desconhecido.</p>;
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-3">
        <Link
          to={`/billing/${kind}`}
          className="inline-flex items-center gap-2 text-sm text-primary hover:underline"
        >
          <ArrowLeft className="size-4" />
          {billingKinds[kind]}
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">
          Detalhe do registro
        </h1>
        <p className="break-all font-mono text-sm text-muted-foreground">
          {id}
        </p>
      </header>
      {record.isLoading && <QueryLoading />}
      {record.error && <QueryError error={record.error} />}
      {replay.error && <QueryError error={replay.error} />}
      {record.data && (
        <>
          <nav
            aria-label="Registros relacionados"
            className="flex flex-wrap gap-3 text-sm"
          >
            {record.data.collection_request_id &&
              ["collections", "attempts", "payments", "webhooks", "outbox"].map(
                (related) => (
                  <Link
                    key={related}
                    className="text-primary hover:underline"
                    to={`/billing/${related}?collection_request_id=${record.data!.collection_request_id}`}
                  >
                    {related}
                  </Link>
                ),
              )}
            {record.data.correlation_id && (
              <Link
                className="text-primary hover:underline"
                to={`/billing/outbox?correlation_id=${record.data.correlation_id}`}
              >
                Outbox da correlação
              </Link>
            )}
          </nav>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-3">
                Estado <Badge variant="outline">{record.data.status}</Badge>
              </CardTitle>
              <CardDescription>
                {record.data.kind} · {formatDate(record.data.occurred_at)}
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Table>
                <TableBody>
                  <DetailRow
                    label="Account"
                    value={record.data.account_id}
                    href={
                      record.data.account_id
                        ? `/accounts/${record.data.account_id}`
                        : undefined
                    }
                  />
                  <DetailRow
                    label="Cobrança"
                    value={record.data.collection_request_id}
                    href={
                      record.data.collection_request_id
                        ? `/billing/collections/${record.data.collection_request_id}`
                        : undefined
                    }
                  />
                  <DetailRow
                    label="Tentativa"
                    value={record.data.collection_attempt_id}
                    href={
                      record.data.collection_attempt_id
                        ? `/billing/attempts/${record.data.collection_attempt_id}`
                        : undefined
                    }
                  />
                  <DetailRow
                    label="Pagamento"
                    value={record.data.billing_payment_id}
                    href={
                      record.data.billing_payment_id
                        ? `/billing/payments/${record.data.billing_payment_id}`
                        : undefined
                    }
                  />
                  <DetailRow
                    label="Correlação"
                    value={record.data.correlation_id}
                  />
                  <DetailRow
                    label="Evento do provedor"
                    value={record.data.provider_event_id}
                  />
                  <DetailRow
                    label="Pagamento do provedor"
                    value={record.data.provider_payment_id}
                  />
                  <DetailRow label="Tipo" value={record.data.event_type} />
                  <DetailRow
                    label="Valor em unidades menores"
                    value={record.data.amount_minor?.toString()}
                  />
                  <DetailRow label="Moeda" value={record.data.currency} />
                  <DetailRow label="Cupom" value={record.data.coupon_code} />
                  <DetailRow label="Preço original (unidades menores)" value={record.data.base_amount_minor?.toString()} />
                  <DetailRow label="Desconto (unidades menores)" value={record.data.discount_amount_minor?.toString()} />
                  <DetailRow
                    label="Código de falha"
                    value={record.data.failure_code}
                  />
                  <DetailRow label="Detalhe" value={record.data.detail} />
                </TableBody>
              </Table>
              {kind === "outbox" && record.data.status === "DEAD_LETTER" && (
                <AlertDialog>
                  <AlertDialogTrigger render={<Button variant="outline" />}>
                    Reenfileirar evento
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Reenfileirar evento?</AlertDialogTitle>
                      <AlertDialogDescription>
                        O evento {id} voltará à fila de entrega. Confira a causa
                        da falha antes de confirmar.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel>Voltar</AlertDialogCancel>
                      <AlertDialogAction
                        disabled={replay.isPending}
                        onClick={() => replay.mutate()}
                      >
                        Reenfileirar
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}
              {replay.data && (
                <p role="status" className="text-sm">
                  Evento reenfileirado.
                </p>
              )}
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}

function DetailRow({
  label,
  value,
  href,
}: {
  label: string;
  value?: string | null;
  href?: string;
}) {
  if (!value) return null;
  return (
    <TableRow>
      <TableCell className="w-48 text-muted-foreground">{label}</TableCell>
      <TableCell className="break-all font-mono text-xs">
        {href ? (
          <Link className="text-primary hover:underline" to={href}>
            {value}
          </Link>
        ) : (
          value
        )}
      </TableCell>
    </TableRow>
  );
}
