import { useState, type FormEvent } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api";

type SetupSession = {
  payment_method_setup_id: string;
  redirect_url: string;
};

export function PaymentMethodSetup({
  onComplete,
  onClose,
}: {
  onComplete: (session: SetupSession, cardName: string) => void;
  onClose: () => void;
}) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function startSetup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const cardName = String(new FormData(event.currentTarget).get("card_name") ?? "").trim();
      const session = await api<SetupSession>("/payment-method-setup", { method: "POST" });
      onComplete(session, cardName);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Não foi possível iniciar a configuração do cartão.");
      setBusy(false);
    }
  }

  return (
    <form className="grid gap-4 rounded-lg border p-4" onSubmit={startSetup}>
      <p className="text-sm text-muted-foreground">
        Você continuará na página segura do provedor. O cartão será salvo para cobranças futuras.
      </p>
      <FieldGroup className="grid gap-3">
        <Field>
          <FieldLabel htmlFor="card-display-name">Nome para identificar o cartão (opcional)</FieldLabel>
          <Input id="card-display-name" name="card_name" placeholder="Ex.: Cartão pessoal" maxLength={50} />
        </Field>
      </FieldGroup>
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <div className="flex flex-wrap gap-2">
        <Button type="submit" variant="outline" disabled={busy}>
          {busy && <Spinner data-icon="inline-start" />}
          {busy ? "Preparando página segura" : "Continuar para página segura"}
        </Button>
        <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
          Cancelar
        </Button>
      </div>
    </form>
  );
}
