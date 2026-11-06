import { ArrowRight, Sparkles } from "lucide-react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { type User } from "@/lib/api"

type PlanModel = Exclude<User["plan_model"], null>
type OnboardingScreenProps = {
  username: string
  busy: boolean
  error: string
  onChoose: (model: PlanModel) => void
}

export function OnboardingScreen({ username, busy, error, onChoose }: OnboardingScreenProps) {
  return <main className="app-shell onboarding-shell">
    <header className="app-header">
      <Brand />
      <Badge variant="secondary">Olá, {username}</Badge>
    </header>
    <section className="onboarding-content">
      <div className="space-y-3">
        <p className="eyebrow">Teste gratuito</p>
        <h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">Sua conta está pronta para começar.</h1>
        <p className="max-w-2xl text-muted-foreground">Você recebe 10 créditos para testar. Cada tarefa concluída consome 1 crédito.</p>
      </div>
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <Button onClick={() => onChoose("PREPAID")} disabled={busy}>
          {busy ? "Preparando…" : "Ativar créditos de teste"}
          <ArrowRight data-icon="inline-end" />
        </Button>
        <p className="text-sm text-muted-foreground">As próximas compras usam checkout hospedado pelo Subscription.</p>
      </div>
    </section>
  </main>
}

function Brand() {
  return <div className="brand-lockup">
    <span className="brand-mark">T</span>
    <span>TaskLab<span className="text-primary">.</span></span>
  </div>
}
