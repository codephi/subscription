import { useState, type FormEvent } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api";

type SetupResult = {
  payment_method_binding_id?: string | null;
  saved: boolean;
};
type CardSetupInput = {
  cardholder_name: string;
  card_name?: string;
  card_number: string;
  exp_month: number;
  exp_year: number;
  cvc: string;
  save_for_future: boolean;
};

export function PaymentMethodSetup({
  onComplete,
  onClose,
}: {
  onComplete: (result: SetupResult) => void;
  onClose: () => void;
}) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);

  async function submitCard(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const card = cardSetupInput(new FormData(event.currentTarget), saved);
      const result = await api<SetupResult>("/payment-method-setup", {
        method: "POST",
        headers: { "idempotency-key": crypto.randomUUID() },
        body: JSON.stringify(card),
      });
      onComplete(result);
    } catch (reason) {
      setError(
        reason instanceof Error
          ? reason.message
          : "Não foi possível validar o cartão.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="grid gap-4 rounded-lg border p-4" onSubmit={submitCard}>
      <FieldGroup className="grid gap-3 sm:grid-cols-2">
        <Field className="sm:col-span-2">
          <FieldLabel htmlFor="cardholder-name">
            Nome impresso no cartão
          </FieldLabel>
          <Input
            id="cardholder-name"
            name="cardholder_name"
            autoComplete="cc-name"
            required
            maxLength={100}
          />
        </Field>
        <Field className="sm:col-span-2">
          <FieldLabel htmlFor="card-number">Número do cartão</FieldLabel>
          <Input
            id="card-number"
            name="card_number"
            inputMode="numeric"
            autoComplete="cc-number"
            pattern="[0-9 -]{12,23}"
            maxLength={23}
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="card-exp-month">Mês de validade</FieldLabel>
          <Input
            id="card-exp-month"
            name="exp_month"
            inputMode="numeric"
            autoComplete="cc-exp-month"
            type="number"
            min={1}
            max={12}
            placeholder="MM"
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="card-exp-year">Ano de validade</FieldLabel>
          <Input
            id="card-exp-year"
            name="exp_year"
            inputMode="numeric"
            autoComplete="cc-exp-year"
            type="number"
            min={new Date().getFullYear()}
            max={new Date().getFullYear() + 20}
            placeholder="AAAA"
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="card-cvc">Código de segurança</FieldLabel>
          <Input
            id="card-cvc"
            name="cvc"
            inputMode="numeric"
            autoComplete="cc-csc"
            type="password"
            minLength={3}
            maxLength={4}
            required
          />
        </Field>
      </FieldGroup>
      <label className="flex items-start gap-2 text-sm text-muted-foreground">
        <input
          type="checkbox"
          checked={saved}
          onChange={(event) => setSaved(event.target.checked)}
        />
        <span>Salvar este cartão para usar em próximas recargas.</span>
      </label>
      {saved && (
        <Field>
          <FieldLabel htmlFor="card-display-name">
            Nome para identificar o cartão (opcional)
          </FieldLabel>
          <Input
            id="card-display-name"
            name="card_name"
            placeholder="Ex.: Cartão pessoal"
            maxLength={50}
          />
        </Field>
      )}
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <p className="text-xs text-muted-foreground">
        Os dados são encaminhados à Subscription para validação no provedor e
        não são armazenados pela TaskLab.
      </p>
      <div className="flex flex-wrap gap-2">
        <Button type="submit" variant="outline" disabled={busy}>
          {busy && <Spinner data-icon="inline-start" />}
          {busy
            ? "Validando cartão"
            : saved
              ? "Validar e salvar cartão"
              : "Validar cartão"}
        </Button>
        <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
          Cancelar
        </Button>
      </div>
    </form>
  );
}

function cardSetupInput(
  form: FormData,
  saveForFuture: boolean,
): CardSetupInput {
  return {
    cardholder_name: String(form.get("cardholder_name") ?? "").trim(),
    card_name: saveForFuture
      ? String(form.get("card_name") ?? "").trim() || undefined
      : undefined,
    card_number: String(form.get("card_number") ?? "").replace(/[ -]/g, ""),
    exp_month: Number(form.get("exp_month")),
    exp_year: Number(form.get("exp_year")),
    cvc: String(form.get("cvc") ?? ""),
    save_for_future: saveForFuture,
  };
}
