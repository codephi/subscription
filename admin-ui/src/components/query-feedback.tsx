import { AlertCircle, Inbox } from "lucide-react";
import { ApiRequestError } from "@/api/client";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";

export function QueryLoading() {
  return (
    <div role="status" aria-label="Carregando" className="flex flex-col gap-3">
      <Skeleton className="h-16 w-full" />
      <Skeleton className="h-16 w-full" />
    </div>
  );
}

export function QueryError({ error }: { error: Error }) {
  const apiError = error instanceof ApiRequestError ? error : null;
  const title =
    apiError?.status === 503
      ? "Dados temporariamente indisponíveis"
      : "Não foi possível carregar os dados";
  return (
    <Alert variant="destructive">
      <AlertCircle aria-hidden="true" />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        {apiError ? `${apiError.code}: ${apiError.message}` : error.message}
        {apiError?.existingOperation && (
          <p className="mt-2 break-all font-mono text-xs">
            Operação existente: {apiError.existingOperation.operation_kind} ·{" "}
            {apiError.existingOperation.resource_id} · transação{" "}
            {apiError.existingOperation.transaction_id}
          </p>
        )}
      </AlertDescription>
    </Alert>
  );
}

export function QueryEmpty({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <Inbox />
        </EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        <EmptyDescription>{description}</EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}
