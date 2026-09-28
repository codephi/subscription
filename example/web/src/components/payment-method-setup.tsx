import { useState } from "react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { api } from "@/lib/api"

type SetupRedirect = { redirect_url: string }

export function PaymentMethodSetup() {
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const [futureUseConsent, setFutureUseConsent] = useState(false)

  async function beginSetup() {
    setBusy(true)
    setError("")
    try {
      const session = await api<SetupRedirect>("/payment-method-setup", { method: "POST", body: "{}" })
      window.location.assign(securePaymentRedirectUrl(session.redirect_url))
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Não foi possível iniciar o cadastro do cartão.")
      setBusy(false)
    }
  }

  return <div className="space-y-3">
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <label className="flex items-start gap-2 text-sm text-muted-foreground">
      <input type="checkbox" checked={futureUseConsent} onChange={(event) => setFutureUseConsent(event.target.checked)} />
      <span>Autorizo salvar este cartão para cobranças futuras da assinatura ou compras que eu iniciar.</span>
    </label>
    <Button type="button" variant="outline" onClick={beginSetup} disabled={busy || !futureUseConsent}>
      {busy && <Spinner data-icon="inline-start" />}Adicionar ou validar cartão
    </Button>
  </div>
}

function securePaymentRedirectUrl(value: string): string {
  const url = new URL(value)
  if (url.protocol !== "https:") {
    throw new Error(`Subscription returned payment URL ${value}; expected an HTTPS URL`)
  }
  return url.href
}
