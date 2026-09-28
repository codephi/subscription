import { useState } from "react"
import { Elements, PaymentElement, useElements, useStripe } from "@stripe/react-stripe-js"
import { loadStripe, type Stripe } from "@stripe/stripe-js"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { api } from "@/lib/api"

type SetupSession = { client_secret: string; publishable_key: string }
type PaymentBinding = { payment_method_binding_id: string; status: string }

export function StripeCardSetup({ onSaved }: { onSaved: (bindingId: string) => void }) {
  const [session, setSession] = useState<SetupSession | null>(null)
  const [stripePromise, setStripePromise] = useState<Promise<Stripe | null> | null>(null)
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)

  async function beginSetup() {
    setBusy(true)
    setError("")
    try {
      const next = await api<SetupSession>("/payment-method-setup", { method: "POST", body: "{}" })
      setStripePromise(loadStripe(next.publishable_key))
      setSession(next)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Não foi possível iniciar a validação do cartão.")
    } finally {
      setBusy(false)
    }
  }

  async function saveSetupIntent(setupIntentId: string) {
    setBusy(true)
    try {
      const binding = await api<PaymentBinding>("/payment-method-bindings", {
        method: "POST",
        body: JSON.stringify({ setup_intent_id: setupIntentId }),
      })
      onSaved(binding.payment_method_binding_id)
      setSession(null)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Não foi possível salvar o cartão validado.")
    } finally {
      setBusy(false)
    }
  }

  return <div className="space-y-3">
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {!session && <Button type="button" variant="outline" onClick={beginSetup} disabled={busy}>
      {busy && <Spinner data-icon="inline-start" />}Adicionar ou validar cartão
    </Button>}
    {session && stripePromise && <Elements stripe={stripePromise} options={{ clientSecret: session.client_secret }}>
      <ConfirmCardSetup busy={busy} setBusy={setBusy} setError={setError} onConfirmed={saveSetupIntent} />
    </Elements>}
  </div>
}

function ConfirmCardSetup({ busy, setBusy, setError, onConfirmed }: {
  busy: boolean
  setBusy: (busy: boolean) => void
  setError: (error: string) => void
  onConfirmed: (setupIntentId: string) => Promise<void>
}) {
  const stripe = useStripe()
  const elements = useElements()

  async function confirmCard(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!stripe || !elements) return
    setBusy(true)
    setError("")
    const result = await stripe.confirmSetup({
      elements,
      confirmParams: { return_url: window.location.href },
      redirect: "if_required",
    })
    if (result.error) {
      setError(result.error.message ?? "A Stripe recusou os dados do cartão.")
      setBusy(false)
      return
    }
    if (result.setupIntent.status !== "succeeded") {
      setError(`SetupIntent ${result.setupIntent.id} terminou com estado ${result.setupIntent.status}.`)
      setBusy(false)
      return
    }
    await onConfirmed(result.setupIntent.id)
  }

  return <form onSubmit={confirmCard} className="space-y-3">
    <PaymentElement />
    <Button type="submit" disabled={!stripe || busy}>
      {busy && <Spinner data-icon="inline-start" />}Validar e salvar cartão
    </Button>
  </form>
}
