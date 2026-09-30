import type { ReactNode } from "react"
import { CreditCard, History, LogOut, Play, Plus, Sparkles, Trash2, WalletCards, Zap } from "lucide-react"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Field, FieldContent, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item"
import { Separator } from "@/components/ui/separator"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import type { Dashboard, PaymentMethodBinding, User } from "@/lib/api"

type CheckoutView = { checkout_id: string; status: string; amount_minor?: number | null }

type WorkspaceDashboardProps = {
  user: User
  view?: Dashboard
  loading: boolean
  queryError?: string
  actionError: string
  retryDashboard: () => void
  checkout: CheckoutView | null
  selectedPaymentMethodId: string
  setSelectedPaymentMethodId: (id: string) => void
  taskName: string
  setTaskName: (name: string) => void
  taskResult: string
  pendingExecution: boolean
  checkoutBusy: boolean
  topupCredits: number
  setTopupCredits: (credits: number) => void
  executionBusy: boolean
  onSignOut: () => void
  onCheckout: (credits: number) => void
  onAddPaymentMethod: () => void
  onRemovePaymentMethod: (bindingId: string) => void
  removingPaymentMethod: boolean
  cardSetupForm: ReactNode
  onExecute: () => void
}

export function WorkspaceDashboard(props: WorkspaceDashboardProps) {
  const { user, view, loading, queryError, actionError, retryDashboard, checkout } = props
  const latestCheckout = view?.checkouts[0]

  return <main className="app-shell">
    <header className="app-header">
      <Brand />
      <div className="flex items-center gap-3">
        <Badge variant="secondary" className="hidden sm:inline-flex">POC de créditos</Badge>
        <span className="text-sm text-muted-foreground">{user.username}</span>
        <Button variant="outline" size="sm" onClick={props.onSignOut}><LogOut data-icon="inline-start" />Sair</Button>
      </div>
    </header>
    <section className="dashboard-heading">
      <div className="space-y-2">
        <p className="eyebrow">Seu espaço de trabalho</p>
        <h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">Olá, {user.username}.</h1>
        <p className="text-muted-foreground">Acompanhe seus créditos e valide os fluxos da Subscription.</p>
      </div>
      <Badge variant="outline" className="gap-2"><span className="connection-dot" />API conectada</Badge>
    </section>
    {actionError && <ErrorNotice message={actionError} />}
    {queryError && <Alert variant="destructive">
      <AlertTitle>Não foi possível carregar o saldo</AlertTitle>
      <AlertDescription className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <span>{queryError}</span>
        <Button variant="outline" size="sm" onClick={retryDashboard}>Tentar novamente</Button>
      </AlertDescription>
    </Alert>}
    {checkout && <CheckoutNotice checkout={checkout} />}
    <div className="dashboard-grid">
      <section className="dashboard-main">
        <BalanceCard user={user} view={view} loading={loading} />
        <ExecutionCard {...props} />
        <ActivityCard view={view} loading={loading} />
      </section>
      <aside className="dashboard-aside">
        <PlanCard user={user} busy={props.checkoutBusy} onCheckout={() => props.onCheckout(props.topupCredits)} topupCredits={props.topupCredits} />
        {(user.plan_model === "PREPAID" || latestCheckout?.status !== "PAID") && <CheckoutCard {...props} />}
        <LedgerCard view={view} loading={loading} />
      </aside>
    </div>
  </main>
}

function Brand() {
  return <div className="brand-lockup">
    <span className="brand-mark">T</span>
    <span>TaskLab<span className="text-primary">.</span></span>
  </div>
}

function BalanceCard({ user, view, loading }: { user: User; view?: Dashboard; loading: boolean }) {
  const balance = view?.eligibility?.balance_credit_units ?? view?.wallet_statement.items[0]?.balance_after_credit_units ?? "0"

  return <Card>
    <CardHeader className="flex items-start justify-between gap-4">
      <div className="space-y-1">
        <CardDescription>Créditos disponíveis</CardDescription>
        <CardTitle className="balance-value">{loading ? <Skeleton className="h-12 w-28" /> : balance}</CardTitle>
      </div>
      <div className="metric-icon"><WalletCards /></div>
    </CardHeader>
    <CardFooter className="flex-wrap justify-between gap-3">
      <Badge variant="outline">{user.plan_model === "PREPAID" ? "Pré-pago" : "Assinatura mensal"}</Badge>
      <span className="text-sm text-muted-foreground">1 execução = 1 crédito</span>
    </CardFooter>
  </Card>
}

function ExecutionCard(props: WorkspaceDashboardProps) {
  const allowed = props.view?.eligibility?.access_allowed === true
  const buttonDisabled = !props.taskName.trim() || props.executionBusy || (!allowed && !props.pendingExecution)

  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    props.onExecute()
  }

  return <Card>
    <CardHeader>
      <CardTitle className="flex items-center gap-2"><Play />Executar tarefa</CardTitle>
      <CardDescription>Uma tarefa custa 1 crédito. A elegibilidade é consultada antes do consumo.</CardDescription>
    </CardHeader>
    <CardContent className="space-y-4">
      <form onSubmit={submit} className="flex flex-col gap-3 sm:flex-row sm:items-end">
        <Field className="flex-1">
          <FieldLabel htmlFor="task-name">Nome da tarefa</FieldLabel>
          <Input id="task-name" placeholder="Ex.: preparar resumo semanal" value={props.taskName} onChange={(event) => props.setTaskName(event.target.value)} disabled={props.pendingExecution} maxLength={120} />
        </Field>
        <Button type="submit" disabled={buttonDisabled}>
          {props.executionBusy ? <Spinner data-icon="inline-start" /> : <Play data-icon="inline-start" />}
          {props.pendingExecution ? "Repetir operação" : "Executar"}
        </Button>
      </form>
      {!allowed && !props.pendingExecution && <p className="text-sm text-muted-foreground">Adicione créditos para liberar a execução.</p>}
      {props.taskResult && <Alert><AlertTitle>Tarefa concluída</AlertTitle><AlertDescription>{props.taskResult}</AlertDescription></Alert>}
    </CardContent>
  </Card>
}

function ActivityCard({ view, loading }: { view?: Dashboard; loading: boolean }) {
  const items = view?.executions ?? []

  return <Card>
    <CardHeader className="flex-row items-center justify-between">
      <div className="space-y-1">
        <CardTitle className="flex items-center gap-2"><History />Atividade</CardTitle>
        <CardDescription>Tarefas concluídas e créditos consumidos.</CardDescription>
      </div>
      <Badge variant="secondary">{items.length}</Badge>
    </CardHeader>
    <CardContent>
      {loading ? <TableSkeleton /> : items.length ? <Table>
        <TableHeader><TableRow><TableHead>Tarefa</TableHead><TableHead>Resultado</TableHead><TableHead className="text-right">Créditos</TableHead></TableRow></TableHeader>
        <TableBody>{items.map((entry) => <TableRow key={entry.execution_id}>
          <TableCell className="font-medium">{entry.task_name}</TableCell>
          <TableCell className="max-w-56 truncate text-muted-foreground">{entry.result_text ?? entry.status}</TableCell>
          <TableCell className="text-right"><Badge variant="outline">−{entry.credits_debited}</Badge></TableCell>
        </TableRow>)}</TableBody>
      </Table> : <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon"><History /></EmptyMedia>
          <EmptyTitle>Nenhuma tarefa ainda</EmptyTitle>
          <EmptyDescription>As execuções concluídas aparecem aqui com o consumo registrado.</EmptyDescription>
        </EmptyHeader>
      </Empty>}
    </CardContent>
  </Card>
}

function PlanCard({ user, busy, onCheckout, topupCredits }: { user: User; busy: boolean; onCheckout: () => void; topupCredits: number }) {
  const prepaid = user.plan_model === "PREPAID"

  return <Card>
    <CardHeader>
      <CardTitle>{prepaid ? "Créditos pré-pagos" : "Plano mensal"}</CardTitle>
      <CardDescription>{prepaid ? "Recarregue sempre que precisar." : "Créditos liberados após cada ciclo confirmado."}</CardDescription>
    </CardHeader>
    <CardContent className="flex items-end justify-between gap-3">
      <div>
        <p className="text-2xl font-semibold tracking-tight">{prepaid ? formatCurrency(topupCredits * 100) : "R$ 29,90"}</p>
        <p className="text-sm text-muted-foreground">{prepaid ? `${topupCredits} créditos por compra` : "50 créditos por ciclo"}</p>
      </div>
      {prepaid ? <Button variant="outline" onClick={onCheckout} disabled={busy}><Plus data-icon="inline-start" />Recarregar</Button> : <Badge variant="secondary">Ativo</Badge>}
    </CardContent>
    {prepaid && <CardFooter className="text-sm text-muted-foreground">Checkout gerenciado pela Subscription.</CardFooter>}
  </Card>
}

function CheckoutCard(props: WorkspaceDashboardProps) {
  const pending = props.checkout?.status === "PENDING"
  const prepaid = props.user.plan_model === "PREPAID"
  const offers = props.view?.catalog.topup_offers ?? [10, 25, 50].map((credit_units) => ({ credit_units, price_amount_minor: credit_units * 100 }))

  return <Card>
    <CardHeader>
      <CardTitle className="flex items-center gap-2"><Sparkles />Simular checkout</CardTitle>
      <CardDescription>Escolha um cartão salvo ou adicione outro na página segura do provedor. A Subscription mantém a referência tokenizada.</CardDescription>
    </CardHeader>
    <CardContent className="space-y-4">
      {prepaid && <Field>
        <FieldLabel>Quanto deseja carregar?</FieldLabel>
        <RadioGroup value={String(props.topupCredits)} onValueChange={(value) => props.setTopupCredits(Number(value))} disabled={props.checkoutBusy || pending} className="grid gap-2 sm:grid-cols-3">
          {offers.map((offer) => <Field data-selected={props.topupCredits === offer.credit_units} className="plan-option" key={offer.credit_units}>
            <RadioGroupItem id={`topup-${offer.credit_units}`} value={String(offer.credit_units)} />
            <FieldContent>
              <FieldLabel htmlFor={`topup-${offer.credit_units}`}>{offer.credit_units} créditos</FieldLabel>
              <FieldDescription>{formatCurrency(offer.price_amount_minor)}</FieldDescription>
            </FieldContent>
          </Field>)}
        </RadioGroup>
      </Field>}
      <SavedPaymentMethods methods={props.view?.payment_methods ?? []} selectedId={props.selectedPaymentMethodId} onSelect={props.setSelectedPaymentMethodId} onRemove={props.onRemovePaymentMethod} removing={props.removingPaymentMethod} disabled={props.checkoutBusy || pending} />
      <Button variant="outline" onClick={props.onAddPaymentMethod} disabled={props.checkoutBusy || pending}>
        <CreditCard data-icon="inline-start" />Adicionar cartão
      </Button>
      {props.cardSetupForm}
      {props.checkoutBusy && <Alert role="status" aria-live="polite">
        <Spinner />
        <AlertTitle>Preparando sua {prepaid ? "recarga" : "assinatura"}</AlertTitle>
        <AlertDescription>Enviando a solicitação para a Subscription. Esta etapa pode levar alguns segundos.</AlertDescription>
      </Alert>}
      <Item variant="muted" size="sm">
        <ItemMedia variant="icon"><CreditCard /></ItemMedia>
        <ItemContent><ItemTitle>Dados protegidos</ItemTitle><ItemDescription>Os dados do cartão são informados na página segura do provedor; TaskLab recebe somente a confirmação do vínculo.</ItemDescription></ItemContent>
      </Item>
    </CardContent>
    <CardFooter>
      <Button className="w-full" variant="outline" onClick={() => props.onCheckout(props.topupCredits)} disabled={props.checkoutBusy || pending || !props.selectedPaymentMethodId}>
        {props.checkoutBusy || pending ? <Spinner data-icon="inline-start" /> : <Zap data-icon="inline-start" />}
        {prepaid ? "Iniciar recarga" : failedCheckout(props.checkout) ? "Tentar assinatura novamente" : "Iniciar assinatura"}
      </Button>
    </CardFooter>
  </Card>
}

function SavedPaymentMethods({ methods, selectedId, onSelect, onRemove, removing, disabled }: { methods: PaymentMethodBinding[]; selectedId: string; onSelect: (id: string) => void; onRemove: (id: string) => void; removing: boolean; disabled: boolean }) {
  const active = methods.filter((method) => method.status === "ACTIVE")
  const selected = active.find((method) => method.payment_method_binding_id === selectedId)
  return <Field>
    <FieldLabel htmlFor="saved-payment-method">Cartão salvo</FieldLabel>
    <div className="flex flex-wrap gap-2">
    <select id="saved-payment-method" className="h-10 min-w-0 flex-1 rounded-md border bg-background px-3 text-sm" value={selectedId} onChange={(event) => onSelect(event.target.value)} disabled={!active.length || disabled || removing}>
      {!active.length && <option value="">Adicione um cartão para continuar</option>}
      {active.map((method, index) => <option key={method.payment_method_binding_id} value={method.payment_method_binding_id}>{method.display_name ?? `Cartão salvo ${index + 1}`}</option>)}
    </select>
    {selected && <Button type="button" variant="outline" onClick={() => onRemove(selected.payment_method_binding_id)} disabled={disabled || removing}>
      {removing ? <Spinner data-icon="inline-start" /> : <Trash2 data-icon="inline-start" />}
      Remover cartão
    </Button>}
    </div>
  </Field>
}

function LedgerCard({ view, loading }: { view?: Dashboard; loading: boolean }) {
  const entries = view?.wallet_statement.items.slice(0, 5) ?? []

  return <Card>
    <CardHeader>
      <CardTitle>Extrato de créditos</CardTitle>
      <CardDescription>Movimentações consultadas na API.</CardDescription>
    </CardHeader>
    <CardContent>
      {loading ? <TableSkeleton rows={3} /> : entries.length ? <Table>
        <TableHeader><TableRow><TableHead>Movimento</TableHead><TableHead className="text-right">Créditos</TableHead></TableRow></TableHeader>
        <TableBody>{entries.map((entry, index) => <TableRow key={`${entry.created_at}-${index}`}>
          <TableCell>
            <span className="block font-medium">{entry.description ?? entry.entry_type}</span>
            <span className="text-xs text-muted-foreground">{formatDate(entry.created_at)}</span>
          </TableCell>
          <TableCell className="text-right font-medium">{entry.signed_credit_units}</TableCell>
        </TableRow>)}</TableBody>
      </Table> : <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon"><WalletCards /></EmptyMedia>
          <EmptyTitle>Sem movimentações</EmptyTitle>
          <EmptyDescription>Recargas e consumos aparecerão neste extrato.</EmptyDescription>
        </EmptyHeader>
      </Empty>}
    </CardContent>
  </Card>
}

function CheckoutNotice({ checkout }: { checkout: CheckoutView }) {
  const paid = checkout.status === "PAID"
  const variant = checkout.status === "FAILED" || checkout.status === "EXPIRED" ? "destructive" : "default"

  return <Alert variant={variant}>
    <CreditCard />
    <AlertTitle>Checkout {checkout.status.toLowerCase()}</AlertTitle>
    <AlertDescription>{paid ? "Pagamento confirmado pela Subscription." : checkout.status === "PENDING" ? "A Subscription está processando; este painel acompanha o resultado." : `Estado atualizado: ${checkout.status}.`}</AlertDescription>
  </Alert>
}

function ErrorNotice({ message }: { message: string }) {
  return <Alert variant="destructive"><AlertTitle>Não foi possível concluir a operação</AlertTitle><AlertDescription>{message}</AlertDescription></Alert>
}

function TableSkeleton({ rows = 3 }: { rows?: number }) {
  return <div className="space-y-3" aria-label="Carregando dados">
    {Array.from({ length: rows }, (_, index) => <Skeleton key={index} className="h-10 w-full" />)}
  </div>
}

function failedCheckout(checkout: CheckoutView | null) {
  return checkout?.status === "FAILED" || checkout?.status === "EXPIRED"
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" }).format(new Date(value))
}

function formatCurrency(amountMinor: number) {
  return new Intl.NumberFormat("pt-BR", { style: "currency", currency: "BRL" }).format(amountMinor / 100)
}
