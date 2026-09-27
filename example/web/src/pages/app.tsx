import { useEffect, useRef, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowRight, Check, CircleHelp, CreditCard, History, LoaderCircle, LogOut, Play, Plus, Sparkles, WalletCards, Zap } from "lucide-react"
import { api, type Dashboard, type User } from "../lib/api"
import { Alert, AlertDescription } from "../components/ui/alert"
import { Badge } from "../components/ui/badge"
import { Button } from "../components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "../components/ui/card"
import { Input } from "../components/ui/input"
import { Label } from "../components/ui/label"

type AuthMode = "login" | "register"
type CheckoutKind = "INITIAL" | "ON_DEMAND"
type CheckoutView = { checkout_id: string; status: string; amount_minor?: number | null }

export default function App() {
  const queryClient = useQueryClient()
  const [user, setUser] = useState<User | null>(null)
  const [authMode, setAuthMode] = useState<AuthMode>("login")
  const [authChecked, setAuthChecked] = useState(false)
  const [error, setError] = useState("")
  const [taskName, setTaskName] = useState("")
  const [pendingExecution, setPendingExecution] = useState(false)
  const [taskResult, setTaskResult] = useState("")
  const [checkout, setCheckout] = useState<CheckoutView | null>(null)
  const [card, setCard] = useState({ number: "", expiry: "", cvc: "" })
  const checkoutKey = useRef<string | null>(null)
  const executionTransaction = useRef<string | null>(null)

  useEffect(() => { api<User>("/me").then(setUser).catch(() => undefined).finally(() => setAuthChecked(true)) }, [])
  useEffect(() => {
    if (!user) return
    const savedExecution = sessionStorage.getItem(`tasklab_execution_${user.username}`)
    if (savedExecution) {
      const operation = JSON.parse(savedExecution) as { transaction_id: string; task_name: string }
      executionTransaction.current = operation.transaction_id
      setTaskName(operation.task_name)
      setPendingExecution(true)
    }
  }, [user?.username])
  const dashboard = useQuery({ queryKey: ["dashboard"], queryFn: () => api<Dashboard>("/dashboard"), enabled: Boolean(user), refetchInterval: checkout?.status === "PENDING" ? 2500 : false })
  useEffect(() => {
    const latest = dashboard.data?.checkouts[0]
    if (latest?.status === "PENDING" && checkout?.checkout_id !== latest.checkout_id) setCheckout(latest)
  }, [dashboard.data?.checkouts, checkout?.checkout_id])
  useEffect(() => {
    if (!checkout || checkout.status !== "PENDING") return
    const timer = window.setInterval(() => api<CheckoutView>(`/checkouts/${checkout.checkout_id}`).then((result) => {
      setCheckout(result)
      if (result.status !== "PENDING") {
        checkoutKey.current = null
        if (user) sessionStorage.removeItem(`tasklab_checkout_${user.username}`)
      }
    }).catch(showError), 2000)
    return () => window.clearInterval(timer)
  }, [checkout?.checkout_id, checkout?.status, user?.username])
  const authMutation = useMutation({ mutationFn: (body: { username: string; password: string }) => api<User>(authMode === "register" ? "/auth/register" : "/auth/login", { method: "POST", body: JSON.stringify(body) }), onSuccess: (account) => { setUser(account); setError("") }, onError: showError })
  const planMutation = useMutation({ mutationFn: (plan_model: string) => api("/plan", { method: "POST", body: JSON.stringify({ plan_model }) }), onSuccess: () => { queryClient.invalidateQueries({ queryKey: ["dashboard"] }); api<User>("/me").then(setUser) }, onError: showError })
  const checkoutMutation = useMutation({ mutationFn: async (input: { checkout_kind: CheckoutKind; idempotencyKey: string }) => {
    // These fields are intentionally local UI state; only the purchase intention is sent.
    void card
    return api<CheckoutView>("/checkouts", { method: "POST", headers: { "idempotency-key": input.idempotencyKey }, body: JSON.stringify({ checkout_kind: input.checkout_kind }) })
  }, onSuccess: (result) => { setCheckout(result); if (result.status !== "PENDING") { checkoutKey.current = null; sessionStorage.removeItem(`tasklab_checkout_${user?.username}`) } api<User>("/me").then(setUser).catch(() => undefined); queryClient.invalidateQueries({ queryKey: ["dashboard"] }); setError("") }, onError: showError })
  const executeMutation = useMutation({ mutationFn: (operation: { task_name: string; transaction_id: string }) => api<{ result_text: string }>("/executions", { method: "POST", body: JSON.stringify(operation) }), onSuccess: (result) => { setTaskResult(result.result_text); setTaskName(""); setPendingExecution(false); executionTransaction.current = null; if (user) sessionStorage.removeItem(`tasklab_execution_${user.username}`); queryClient.invalidateQueries({ queryKey: ["dashboard"] }); setError("") }, onError: showError })

  function showError(reason: Error) { setError(reason.message) }
  function startCheckout(checkout_kind: CheckoutKind) {
    if (!user) return
    checkoutKey.current ??= sessionStorage.getItem(`tasklab_checkout_${user.username}`)
    checkoutKey.current ??= crypto.randomUUID()
    sessionStorage.setItem(`tasklab_checkout_${user.username}`, checkoutKey.current)
    checkoutMutation.mutate({ checkout_kind, idempotencyKey: checkoutKey.current })
  }
  function startTask() {
    if (!user) return
    let task = taskName
    if (!executionTransaction.current) {
      const saved = sessionStorage.getItem(`tasklab_execution_${user.username}`)
      if (saved) {
        const previous = JSON.parse(saved) as { transaction_id: string; task_name: string }
        executionTransaction.current = previous.transaction_id
        task = previous.task_name
        setTaskName(previous.task_name)
      }
    }
    executionTransaction.current ??= crypto.randomUUID()
    setPendingExecution(true)
    sessionStorage.setItem(`tasklab_execution_${user.username}`, JSON.stringify({ transaction_id: executionTransaction.current, task_name: task }))
    executeMutation.mutate({ task_name: task, transaction_id: executionTransaction.current })
  }
  async function signOut() { await api("/auth/logout", { method: "POST" }).catch(() => undefined); if (user) { sessionStorage.removeItem(`tasklab_checkout_${user.username}`); sessionStorage.removeItem(`tasklab_execution_${user.username}`) } setUser(null); setCheckout(null); queryClient.clear() }

  if (!authChecked) return <main className="shell muted">Abrindo seu espaço…</main>
  if (!user) return <AuthCard mode={authMode} setMode={setAuthMode} onSubmit={(username, password) => authMutation.mutate({ username, password })} busy={authMutation.isPending} error={error} />
  if (!user.plan_model) return <Onboarding username={user.username} card={card} setCard={setCard} onChoose={(model) => model === "PREPAID" ? planMutation.mutate(model) : startCheckout("INITIAL")} busy={planMutation.isPending || checkoutMutation.isPending} error={error} />
  const view = dashboard.data
  const latestCheckout = view?.checkouts[0]
  const subscriptionCanCheckout = user.plan_model === "SUBSCRIPTION" && latestCheckout?.status !== "PAID"
  const balance = view?.eligibility?.balance_credit_units ?? view?.wallet_statement.items[0]?.balance_after_credit_units ?? "0"
  return <main className="shell">
    <header className="nav"><div className="row" style={{ justifyContent: "start" }}><span className="brand">TaskLab<span style={{ color: "#83a58c" }}>.</span></span><Badge variant="secondary">POC de créditos</Badge></div><div className="row"><span className="muted">{user.username}</span><Button variant="outline" size="sm" onClick={signOut}><LogOut />Sair</Button></div></header>
    {error && <Alert variant="destructive" className="mb-5"><CircleHelp /><AlertDescription>{error}</AlertDescription></Alert>}
    <div className="row" style={{ alignItems: "end", marginBottom: 24 }}><div><div className="eyebrow">Seu espaço de trabalho</div><h1 className="display" style={{ margin: "10px 0 0" }}>Bom dia, {user.username}.</h1></div><span className="muted"><span className="status-dot" />API conectada</span></div>
    {checkout && <Alert className="mb-5"><CreditCard /><AlertDescription>Checkout {checkout.status.toLowerCase()}. {checkout.status === "PENDING" ? "A Subscription está processando; este painel acompanha o resultado." : `Estado atualizado: ${checkout.status}.`}</AlertDescription></Alert>}
    <div className="grid-main">
      <div className="stack">
        <Card className="surface"><CardHeader><div className="row"><div><CardDescription>Créditos disponíveis</CardDescription><CardTitle className="stat">{view ? balance : <LoaderCircle className="animate-spin" />}</CardTitle></div><div style={{ background: "#edf5ee", color: "#39744e", padding: 13, borderRadius: 14 }}><WalletCards /></div></div></CardHeader><CardFooter className="row"><Badge variant="outline">{user.plan_model === "PREPAID" ? "Pré-pago" : "Assinatura mensal"}</Badge><span className="muted">1 execução = 1 crédito</span></CardFooter></Card>
        <Card className="surface"><CardHeader><CardTitle>Executar tarefa</CardTitle><CardDescription>Informe um nome. A TaskLab consulta a elegibilidade e registra o consumo na Subscription.</CardDescription></CardHeader><CardContent className="stack"><Label htmlFor="task-name">Nome da tarefa</Label><div className="row"><Input id="task-name" placeholder="Ex.: preparar resumo semanal" value={taskName} disabled={pendingExecution} onChange={(event) => setTaskName(event.target.value)} onKeyDown={(event) => event.key === "Enter" && taskName.trim() && startTask()} /><Button disabled={!taskName.trim() || executeMutation.isPending || (!view?.eligibility?.access_allowed && !pendingExecution)} onClick={startTask}><Play />{pendingExecution ? "Repetir operação" : "Executar"}</Button></div>{!view?.eligibility?.access_allowed && !pendingExecution && <p className="muted" style={{ margin: 0 }}>Adicione créditos para liberar a execução.</p>}{taskResult && <Alert><Check /><AlertDescription>{taskResult}</AlertDescription></Alert>}</CardContent></Card>
        <Card className="surface"><CardHeader><CardTitle className="row"><span className="row" style={{ justifyContent: "start" }}><History />Atividade</span><Badge variant="secondary">{view?.executions.length ?? 0}</Badge></CardTitle></CardHeader><CardContent className="stack">{view?.executions.length ? view.executions.map((entry) => <div className="row" key={entry.execution_id}><div><strong>{entry.task_name}</strong><div className="muted" style={{ fontSize: 13 }}>{entry.result_text ?? entry.status}</div></div><Badge variant="outline">−{entry.credits_debited}</Badge></div>) : <p className="muted">Suas tarefas concluídas aparecerão aqui.</p>}</CardContent></Card>
      </div>
      <aside className="stack">
        {user.plan_model === "PREPAID" ? <Card className="surface"><CardHeader><CardTitle>Recarregar créditos</CardTitle><CardDescription>Pacote fixo para manter as simulações simples.</CardDescription></CardHeader><CardContent><div className="row"><div><div className="stat" style={{ fontSize: 28 }}>R$ 10,00</div><span className="muted">10 créditos</span></div><Button onClick={() => startCheckout("ON_DEMAND")} disabled={checkoutMutation.isPending}>{checkoutMutation.isPending ? <LoaderCircle className="animate-spin" /> : <Plus />}Recarregar</Button></div></CardContent><CardFooter><span className="muted" style={{ fontSize: 12 }}>Checkout seguro gerenciado pela Subscription.</span></CardFooter></Card> : <Card className="surface"><CardHeader><CardTitle>Plano mensal</CardTitle><CardDescription>Créditos liberados após a confirmação de cada ciclo.</CardDescription></CardHeader><CardContent><div className="row"><div><div className="stat" style={{ fontSize: 28 }}>R$ 29,90</div><span className="muted">50 créditos por ciclo</span></div><Badge variant="secondary">Ativo</Badge></div></CardContent></Card>}
        {(user.plan_model === "PREPAID" || subscriptionCanCheckout) && <Card className="surface"><CardHeader><CardTitle className="row" style={{ justifyContent: "start" }}><Sparkles />Simular checkout</CardTitle><CardDescription>Campos cenográficos ficam apenas nesta tela. A aprovação ou recusa é configurada na Subscription.</CardDescription></CardHeader><CardContent className="stack"><div><Label htmlFor="card-number">Cartão de demonstração</Label><Input id="card-number" autoComplete="off" inputMode="numeric" placeholder="0000 0000 0000 0000" value={card.number} onChange={(event) => setCard({ ...card, number: event.target.value })} /></div><div className="row"><Input aria-label="Validade fictícia" placeholder="MM/AA" value={card.expiry} onChange={(event) => setCard({ ...card, expiry: event.target.value })} /><Input aria-label="Código fictício" placeholder="CVC" value={card.cvc} onChange={(event) => setCard({ ...card, cvc: event.target.value })} /></div><Button variant="outline" onClick={() => startCheckout(user.plan_model === "PREPAID" ? "ON_DEMAND" : "INITIAL")} disabled={checkoutMutation.isPending || latestCheckout?.status === "PENDING"}><Zap />{user.plan_model === "PREPAID" ? "Iniciar recarga" : latestCheckout?.status === "FAILED" || latestCheckout?.status === "EXPIRED" ? "Tentar assinatura novamente" : "Iniciar assinatura"}</Button></CardContent></Card>}
        <Card className="surface"><CardHeader><CardTitle>Extrato</CardTitle><CardDescription>Dados consultados na API de créditos.</CardDescription></CardHeader><CardContent className="stack">{view?.wallet_statement.items.slice(0, 5).map((entry, index) => <div className="row" key={`${entry.created_at}-${index}`}><div><strong style={{ fontSize: 13 }}>{entry.description ?? entry.entry_type}</strong><div className="muted" style={{ fontSize: 12 }}>{new Date(entry.created_at).toLocaleString("pt-BR")}</div></div><span>{entry.signed_credit_units}</span></div>) ?? <span className="muted">Carregando extrato…</span>}</CardContent></Card>
      </aside>
    </div>
  </main>
}

function AuthCard({ mode, setMode, onSubmit, busy, error }: { mode: AuthMode; setMode: (mode: AuthMode) => void; onSubmit: (username: string, password: string) => void; busy: boolean; error: string }) {
  const [username, setUsername] = useState("admin")
  const [password, setPassword] = useState("admin")
  return <main className="shell"><Card className="auth surface"><CardHeader><span className="brand">TaskLab<span style={{ color: "#83a58c" }}>.</span></span><div className="eyebrow" style={{ marginTop: 26 }}>Simulador de produto</div><CardTitle className="display" style={{ fontSize: 34, marginTop: 5 }}>{mode === "login" ? "Entre no seu espaço." : "Crie seu espaço."}</CardTitle><CardDescription>Uma POC pequena para testar créditos e assinatura com a Subscription.</CardDescription></CardHeader><CardContent className="stack"><div><Label htmlFor="username">Usuário</Label><Input id="username" value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" /></div><div><Label htmlFor="password">Senha</Label><Input id="password" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete={mode === "login" ? "current-password" : "new-password"} /></div>{error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}<Button className="w-full" onClick={() => onSubmit(username, password)} disabled={busy}>{busy ? <LoaderCircle className="animate-spin" /> : null}{mode === "login" ? "Entrar" : "Criar conta"}<ArrowRight /></Button></CardContent><CardFooter className="muted">{mode === "login" ? "Primeiro acesso?" : "Já tem uma conta?"}<Button variant="link" onClick={() => setMode(mode === "login" ? "register" : "login")}>{mode === "login" ? "Criar conta" : "Entrar"}</Button><span className="muted" style={{ marginLeft: "auto", fontSize: 12 }}>demo: admin / admin</span></CardFooter></Card></main>
}

function Onboarding({ username, card, setCard, onChoose, busy, error }: { username: string; card: { number: string; expiry: string; cvc: string }; setCard: (value: typeof card) => void; onChoose: (model: "PREPAID" | "SUBSCRIPTION") => void; busy: boolean; error: string }) {
  return <main className="shell"><header className="nav"><span className="brand">TaskLab<span style={{ color: "#83a58c" }}>.</span></span><span className="muted">Olá, {username}</span></header><div style={{ maxWidth: 760, margin: "6vh auto" }}><div className="eyebrow">Vamos configurar sua conta</div><h1 className="display" style={{ margin: "12px 0" }}>Como você quer usar a TaskLab?</h1><p className="muted" style={{ marginBottom: 26 }}>Essa escolha define a oferta inicial. Cada tarefa concluída consome 1 crédito.</p>{error && <Alert variant="destructive" className="mb-5"><AlertDescription>{error}</AlertDescription></Alert>}<div className="grid-main"><Card className="surface"><CardHeader><WalletCards style={{ color: "#4d8a61" }} /><CardTitle>Pré-pago</CardTitle><CardDescription>Compre créditos quando precisar e acompanhe cada recarga.</CardDescription></CardHeader><CardContent><div className="stat" style={{ fontSize: 30 }}>R$ 10,00</div><span className="muted">por 10 créditos</span></CardContent><CardFooter><Button className="w-full" onClick={() => onChoose("PREPAID")} disabled={busy}>Escolher pré-pago<ArrowRight /></Button></CardFooter></Card><Card className="surface"><CardHeader><Sparkles style={{ color: "#4d8a61" }} /><CardTitle>Assinatura</CardTitle><CardDescription>Receba créditos a cada ciclo confirmado da assinatura mensal.</CardDescription></CardHeader><CardContent className="stack"><div><div className="stat" style={{ fontSize: 30 }}>R$ 29,90</div><span className="muted">50 créditos por mês</span></div><Label htmlFor="first-card">Cartão fictício (opcional)</Label><Input id="first-card" autoComplete="off" inputMode="numeric" placeholder="0000 0000 0000 0000" value={card.number} onChange={(event) => setCard({ ...card, number: event.target.value })} /><div className="row"><Input aria-label="Validade fictícia" placeholder="MM/AA" value={card.expiry} onChange={(event) => setCard({ ...card, expiry: event.target.value })} /><Input aria-label="Código fictício" placeholder="CVC" value={card.cvc} onChange={(event) => setCard({ ...card, cvc: event.target.value })} /></div><p className="muted" style={{ margin: 0, fontSize: 12 }}>A Subscription controla o método e o resultado do checkout.</p></CardContent><CardFooter><Button variant="outline" className="w-full" onClick={() => onChoose("SUBSCRIPTION")} disabled={busy}>Assinar mensal<ArrowRight /></Button></CardFooter></Card></div>{busy && <p className="muted">Preparando sua conta e o checkout…</p>}</div></main>
}
