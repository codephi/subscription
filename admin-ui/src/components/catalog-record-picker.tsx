import type { FormEvent, ReactNode } from "react";
import { Link } from "react-router-dom";
import { QueryError } from "@/components/query-feedback";
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
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from "@/components/ui/combobox";
import type { SelectableEntry } from "@/lib/catalog-entry";

export function RecordPicker({
  label,
  entries,
  value,
  onChange,
  required = false,
}: {
  label: string;
  entries: SelectableEntry[];
  value: string;
  onChange: (value: string) => void;
  required?: boolean;
}) {
  const selected = entries.find((entry) => entry.id === value) ?? null;
  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Combobox
        items={entries}
        value={selected}
        onValueChange={(entry) => onChange(entry?.id ?? "")}
        itemToStringValue={(entry) => entry.label}
        isItemEqualToValue={(a, b) => a.id === b.id}
      >
        <ComboboxInput
          aria-label={label}
          placeholder={`Buscar ${label.toLowerCase()}`}
          showClear
        />
        <ComboboxContent>
          <ComboboxEmpty>Nenhum resultado.</ComboboxEmpty>
          <ComboboxList>
            {(entry: SelectableEntry) => (
              <ComboboxItem key={entry.id} value={entry}>
                {entry.label}
              </ComboboxItem>
            )}
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
      {required && !value && (
        <FieldDescription>Selecione uma opção para continuar.</FieldDescription>
      )}
    </Field>
  );
}

export function LinkedCreateShell({
  kind,
  onSubmit,
  pending,
  error,
  children,
}: {
  kind: "items" | "prices";
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  pending: boolean;
  error: Error | null;
  children: ReactNode;
}) {
  const title = kind === "items" ? "Criar item" : "Criar preço";
  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6">
      <header className="flex flex-col gap-2">
        <Link
          className="text-sm text-primary hover:underline"
          to={`/catalog/${kind}`}
        >
          ← {kind === "items" ? "Itens" : "Preços"}
        </Link>
        <h1 className="text-3xl font-semibold tracking-tight">{title}</h1>
        <p className="text-muted-foreground">
          Selecione os vínculos pelo nome; o preço será criado como rascunho.
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>{title}</CardTitle>
          <CardDescription>
            Os identificadores são preenchidos automaticamente.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={onSubmit} className="flex flex-col gap-5">
            <FieldGroup>{children}</FieldGroup>
            {error && <QueryError error={error} />}
            <Button type="submit" disabled={pending}>
              {pending ? "Salvando…" : "Criar e conferir"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
