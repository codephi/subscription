import { useState } from "react"
import { ArrowRight, Sparkles, WalletCards } from "lucide-react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldContent, FieldDescription, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { type User } from "@/lib/api"

type PlanModel = Exclude<User["plan_model"], null>
type OnboardingScreenProps = {
  username: string
  selectedPaymentMethodId: string
  busy: boolean
  error: string
  cardSetupForm: React.ReactNode
  onAddPaymentMethod: () => void
  onChoose: (model: PlanModel) => void
}

export function OnboardingScreen({ username, selectedPaymentMethodId, busy, error, cardSetupForm, onAddPaymentMethod, onChoose }: OnboardingScreenProps) {
  const [plan, setPlan] = useState<PlanModel>("PREPAID")

  return <main className="app-shell onboarding-shell">
    <header className="app-header">
      <Brand />
      <Badge variant="secondary">Olá, {username}</Badge>
    </header>
    <section className="onboarding-content">
      <div className="space-y-3">
        <p className="eyebrow">Configure sua conta</p>
        <h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">Como você quer usar a TaskLab?</h1>
        <p className="max-w-2xl text-muted-foreground">Escolha uma modalidade para começar. Cada tarefa concluída consome 1 crédito.</p>
      </div>
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      <FieldSet>
        <FieldLegend>Modalidade de cobrança</FieldLegend>
        <RadioGroup value={plan} onValueChange={(value) => setPlan(value as PlanModel)} disabled={busy} className="plan-options">
          <PlanOption id="plan-prepaid" value="PREPAID" selected={plan === "PREPAID"} title="Pré-pago" description="Compre créditos em pacotes quando precisar." icon={<WalletCards />} price="R$ 10,00" allowance="10 créditos por recarga" />
          <PlanOption id="plan-subscription" value="SUBSCRIPTION" selected={plan === "SUBSCRIPTION"} title="Assinatura" description="Créditos entram após a confirmação do ciclo." icon={<Sparkles />} price="R$ 29,90" allowance="50 créditos por mês" />
        </RadioGroup>
      </FieldSet>
      {plan === "SUBSCRIPTION" && <div className="space-y-2">
        <p className="text-sm text-muted-foreground">{selectedPaymentMethodId ? "Cartão salvo para cobranças futuras." : "Adicione um cartão antes de iniciar a assinatura."}</p>
        <Button type="button" variant="outline" onClick={onAddPaymentMethod}>Adicionar cartão</Button>
        {cardSetupForm}
      </div>}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <Button onClick={() => onChoose(plan)} disabled={busy}>
          {plan === "PREPAID" ? "Começar no pré-pago" : "Iniciar assinatura"}
          <ArrowRight data-icon="inline-end" />
        </Button>
        <p className="text-sm text-muted-foreground">{busy ? "Preparando a conta e o checkout…" : "O pagamento e o resultado são gerenciados pela Subscription."}</p>
      </div>
    </section>
  </main>
}

function PlanOption({ id, value, selected, title, description, icon, price, allowance }: {
  id: string
  value: PlanModel
  selected: boolean
  title: string
  description: string
  icon: React.ReactNode
  price: string
  allowance: string
}) {
  return <Field data-selected={selected} className="plan-option">
    <RadioGroupItem id={id} value={value} />
    <FieldContent>
      <div className="flex items-center gap-2">
        <FieldLabel htmlFor={id}>{title}</FieldLabel>
        {selected && <Badge variant="secondary">Selecionado</Badge>}
      </div>
      <FieldDescription>{description}</FieldDescription>
      <div className="mt-3 flex items-baseline gap-2">
        <span className="text-2xl font-semibold tracking-tight">{price}</span>
        <span className="text-sm text-muted-foreground">{allowance}</span>
      </div>
    </FieldContent>
    <span className="plan-option-icon" aria-hidden="true">{icon}</span>
  </Field>
}

function Brand() {
  return <div className="brand-lockup">
    <span className="brand-mark">T</span>
    <span>TaskLab<span className="text-primary">.</span></span>
  </div>
}
