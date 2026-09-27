import { useState } from "react"
import { ArrowRight, CreditCard, Sparkles, WalletCards } from "lucide-react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { type User } from "@/lib/api"

type PlanModel = Exclude<User["plan_model"], null>
type DemoCard = { number: string; expiry: string; cvc: string }

type OnboardingScreenProps = {
  username: string
  card: DemoCard
  setCard: (card: DemoCard) => void
  busy: boolean
  error: string
  onChoose: (model: PlanModel) => void
}

export function OnboardingScreen({ username, card, setCard, busy, error, onChoose }: OnboardingScreenProps) {
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
      {plan === "SUBSCRIPTION" && <DemoCardFields card={card} setCard={setCard} />}
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

function DemoCardFields({ card, setCard }: { card: DemoCard; setCard: (card: DemoCard) => void }) {
  return <Card>
    <CardHeader>
      <CardTitle className="flex items-center gap-2"><CreditCard />Cartão fictício</CardTitle>
      <CardDescription>Os campos são apenas cenográficos e permanecem no navegador.</CardDescription>
    </CardHeader>
    <CardContent>
      <FieldGroup className="demo-card-fields">
        <Field>
          <FieldLabel htmlFor="first-card-number">Número do cartão</FieldLabel>
          <Input id="first-card-number" autoComplete="off" inputMode="numeric" placeholder="0000 0000 0000 0000" value={card.number} onChange={(event) => setCard({ ...card, number: event.target.value })} />
        </Field>
        <Field>
          <FieldLabel htmlFor="first-card-expiry">Validade fictícia</FieldLabel>
          <Input id="first-card-expiry" autoComplete="off" placeholder="MM/AA" value={card.expiry} onChange={(event) => setCard({ ...card, expiry: event.target.value })} />
        </Field>
        <Field>
          <FieldLabel htmlFor="first-card-cvc">Código fictício</FieldLabel>
          <Input id="first-card-cvc" autoComplete="off" inputMode="numeric" placeholder="CVC" value={card.cvc} onChange={(event) => setCard({ ...card, cvc: event.target.value })} />
        </Field>
      </FieldGroup>
    </CardContent>
    <CardFooter className="text-sm text-muted-foreground">A Subscription controla o método de pagamento e o resultado do checkout.</CardFooter>
  </Card>
}

function Brand() {
  return <div className="brand-lockup">
    <span className="brand-mark">T</span>
    <span>TaskLab<span className="text-primary">.</span></span>
  </div>
}
