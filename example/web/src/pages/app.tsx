import { useEffect, useRef, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { api, type Dashboard, type User } from "@/lib/api"
import { AuthScreen } from "@/components/auth-screen"
import { OnboardingScreen } from "@/components/onboarding-screen"
import { WorkspaceDashboard } from "@/components/workspace-dashboard"
import { LoadingScreen } from "@/components/loading-screen"
import { AppFrame } from "@/components/app-frame"
import { PaymentMethodSetup } from "@/components/payment-method-setup"

type PlanModel = "PREPAID" | "SUBSCRIPTION"
type CheckoutKind = "INITIAL" | "ON_DEMAND"
type CheckoutView = { checkout_id: string; status: string; amount_minor?: number | null; transaction_id?: string }
type CheckoutKey = { scope: string; value: string }

export default function App() {
  const queryClient = useQueryClient()
  const [user, setUser] = useState<User | null>(null)
  const [authChecked, setAuthChecked] = useState(false)
  const [error, setError] = useState("")
  const [taskName, setTaskName] = useState("")
  const [pendingExecution, setPendingExecution] = useState(false)
  const [taskResult, setTaskResult] = useState("")
  const [checkout, setCheckout] = useState<CheckoutView | null>(null)
  const [paymentMethodBindingId, setPaymentMethodBindingId] = useState<string | null>(null)
  const [showCardSetup, setShowCardSetup] = useState(false)
  const [topupCredits, setTopupCredits] = useState(10)
  const checkoutKey = useRef<CheckoutKey | null>(null)
  const executionTransaction = useRef<string | null>(null)

  useEffect(() => restoreSession(setUser, setAuthChecked), [])
  useEffect(() => restorePendingExecution(user, setTaskName, setPendingExecution, executionTransaction), [user?.username])
  useEffect(() => recoverPaymentSetup(user, queryClient, setPaymentMethodBindingId, setUser, setError), [user?.username])

  const dashboard = useQuery({
    queryKey: ["dashboard", user?.username],
    queryFn: () => api<Dashboard>("/dashboard"),
    enabled: Boolean(user),
    refetchInterval: checkout?.status === "PENDING" ? 2500 : false,
  })

  useEffect(() => syncPendingCheckout(dashboard.data, checkout, setCheckout), [dashboard.data?.checkouts, checkout?.checkout_id])
  useEffect(() => {
    if (!paymentMethodBindingId) {
      const firstActive = dashboard.data?.payment_methods?.find((binding) => binding.status === "ACTIVE")
      if (firstActive) setPaymentMethodBindingId(firstActive.payment_method_binding_id)
    }
  }, [dashboard.data?.payment_methods, paymentMethodBindingId])
  useEffect(() => watchCheckout(checkout, user, setCheckout, checkoutKey, setError), [checkout?.checkout_id, checkout?.status, user?.username])
  useEffect(() => {
    if (checkout?.status !== "PAID" || !user) return
    void queryClient.invalidateQueries({ queryKey: ["dashboard", user.username] })
  }, [checkout?.checkout_id, checkout?.status, queryClient, user?.username])

  const authMutation = useMutation({
    mutationFn: submitCredentials,
    onSuccess: (account) => { setUser(account); setError("") },
    onError: reportError(setError),
  })
  const planMutation = useMutation({
    mutationFn: (planModel: PlanModel) => api("/plan", { method: "POST", body: JSON.stringify({ plan_model: planModel }) }),
    onSuccess: async () => refreshAccount(queryClient, setUser),
    onError: reportError(setError),
  })
  const checkoutMutation = useMutation({
    mutationFn: submitCheckout,
    onSuccess: (result) => refreshAfterCheckout(result, user, setUser, setCheckout, checkoutKey, queryClient, setError),
    onError: reportError(setError),
  })
  const executeMutation = useMutation({
    mutationFn: submitExecution,
    onSuccess: (result) => finishExecution(result, user, setTaskResult, setTaskName, setPendingExecution, executionTransaction, queryClient, setError),
    onError: reportError(setError),
  })

  if (!authChecked) return <AppFrame><LoadingScreen /></AppFrame>
  if (!user) return <AppFrame><AuthScreen busy={authMutation.isPending} error={error} onErrorClear={() => setError("")} onSubmit={(mode, credentials) => authMutation.mutate({ mode, ...credentials })} /></AppFrame>
  if (!user.plan_model) {
    return <AppFrame><OnboardingScreen
      username={user.username}
      selectedPaymentMethodId={paymentMethodBindingId ?? ""}
      onAddPaymentMethod={() => setShowCardSetup(true)}
      busy={planMutation.isPending || checkoutMutation.isPending}
      error={error}
      onChoose={(model) => model === "PREPAID" ? planMutation.mutate(model) : paymentMethodBindingId
        ? startCheckout("INITIAL", user, 10, paymentMethodBindingId, checkoutKey, checkoutMutation.mutate)
        : setError("Adicione e valide um cartão antes de iniciar a assinatura.")}
    />{showCardSetup && <PaymentMethodSetup />}</AppFrame>
  }

  return <AppFrame><WorkspaceDashboard
    user={user}
    view={dashboard.data}
    loading={dashboard.isPending}
    queryError={dashboard.error?.message}
    actionError={error}
    retryDashboard={() => dashboard.refetch()}
    checkout={checkout}
    selectedPaymentMethodId={paymentMethodBindingId ?? ""}
    setSelectedPaymentMethodId={setPaymentMethodBindingId}
    topupCredits={topupCredits}
    setTopupCredits={setTopupCredits}
    taskName={taskName}
    setTaskName={setTaskName}
    taskResult={taskResult}
    pendingExecution={pendingExecution}
    checkoutBusy={checkoutMutation.isPending}
    executionBusy={executeMutation.isPending}
    onSignOut={() => signOut(user, queryClient, setUser, setCheckout, setError, checkoutKey, executionTransaction)}
    onAddPaymentMethod={() => setShowCardSetup(true)}
    onCheckout={(credits) => paymentMethodBindingId
      ? startCheckout(user.plan_model === "PREPAID" ? "ON_DEMAND" : "INITIAL", user, credits, paymentMethodBindingId, checkoutKey, checkoutMutation.mutate)
      : setError("Adicione e valide um cartão antes de iniciar o checkout.")}
    onExecute={() => startExecution(user, taskName, setPendingExecution, executionTransaction, executeMutation.mutate)}
  />{showCardSetup && <PaymentMethodSetup />}</AppFrame>
}

function restoreSession(setUser: (user: User | null) => void, setChecked: (checked: boolean) => void) {
  let active = true
  api<User>("/me").then((account) => active && setUser(account)).catch(() => undefined).finally(() => active && setChecked(true))
  return () => { active = false }
}

function recoverPaymentSetup(user: User | null, queryClient: ReturnType<typeof useQueryClient>, setBinding: (id: string) => void, setUser: (user: User) => void, setError: (message: string) => void) {
  const url = new URL(window.location.href)
  const paymentMethodSetupId = url.searchParams.get("payment_method_setup_id")
  if (!user || !paymentMethodSetupId) return
  url.searchParams.delete("payment_method_setup_id")
  url.searchParams.delete("payment_setup")
  window.history.replaceState({}, "", url)
  void api<{ payment_method_binding_id: string }>("/payment-method-bindings", {
    method: "POST",
    body: JSON.stringify({ payment_method_setup_id: paymentMethodSetupId }),
  }).then((binding) => {
    setBinding(binding.payment_method_binding_id)
    setError("")
    return Promise.all([
      queryClient.invalidateQueries({ queryKey: ["dashboard", user.username] }),
      api<User>("/me").then(setUser),
    ])
  }).catch((error: Error) => setError(error.message))
}

function restorePendingExecution(user: User | null, setTask: (task: string) => void, setPending: (pending: boolean) => void, transaction: React.MutableRefObject<string | null>) {
  if (!user) return
  const saved = sessionStorage.getItem(`tasklab_execution_${user.username}`)
  if (!saved) return
  const operation = JSON.parse(saved) as { transaction_id: string; task_name: string }
  transaction.current = operation.transaction_id
  setTask(operation.task_name)
  setPending(true)
}

function submitCredentials(input: { mode: "login" | "register"; username: string; password: string }) {
  const path = input.mode === "register" ? "/auth/register" : "/auth/login"
  return api<User>(path, { method: "POST", body: JSON.stringify({ username: input.username, password: input.password }) })
}

function reportError(setError: (message: string) => void) {
  return (reason: Error) => setError(reason.message)
}

function syncPendingCheckout(view: Dashboard | undefined, checkout: CheckoutView | null, setCheckout: (checkout: CheckoutView) => void) {
  const latest = view?.checkouts[0]
  if (latest?.status === "PENDING" && latest.checkout_id !== checkout?.checkout_id) setCheckout(latest)
}

function watchCheckout(checkout: CheckoutView | null, user: User | null, update: (checkout: CheckoutView) => void, key: React.MutableRefObject<CheckoutKey | null>, fail: (message: string) => void) {
  if (!checkout || checkout.status !== "PENDING") return
  const timer = window.setInterval(() => api<CheckoutView>(`/checkouts/${checkout.checkout_id}`).then((result) => {
    update(result)
    clearFinishedCheckout(result, user, key)
  }).catch((error: Error) => fail(error.message)), 2000)
  return () => window.clearInterval(timer)
}

function clearFinishedCheckout(result: CheckoutView, user: User | null, key: React.MutableRefObject<CheckoutKey | null>) {
  if (result.status === "PENDING") return
  if (user && key.current && (!result.transaction_id || key.current.value === result.transaction_id)) {
    sessionStorage.removeItem(`tasklab_checkout_${user.username}_${key.current.scope}`)
    key.current = null
  }
}

function startCheckout(kind: CheckoutKind, user: User, credits: number, paymentMethodBindingId: string, key: React.MutableRefObject<CheckoutKey | null>, mutate: (input: { checkout_kind: CheckoutKind; topup_credits: number; payment_method_binding_id: string; idempotencyKey: string }) => void) {
  const scope = `${kind}_${kind === "ON_DEMAND" ? credits : "initial"}`
  if (key.current?.scope !== scope) {
    const storageKey = `tasklab_checkout_${user.username}_${scope}`
    key.current = { scope, value: sessionStorage.getItem(storageKey) ?? crypto.randomUUID() }
    sessionStorage.setItem(storageKey, key.current.value)
  }
  mutate({ checkout_kind: kind, topup_credits: credits, payment_method_binding_id: paymentMethodBindingId, idempotencyKey: key.current.value })
}

async function refreshAccount(queryClient: ReturnType<typeof useQueryClient>, setUser: (user: User) => void) {
  await queryClient.invalidateQueries({ queryKey: ["dashboard"] })
  setUser(await api<User>("/me"))
}

function submitCheckout(input: { checkout_kind: CheckoutKind; topup_credits: number; payment_method_binding_id: string; idempotencyKey: string }) {
  const intent = input.checkout_kind === "ON_DEMAND"
    ? { checkout_kind: input.checkout_kind, topup_credits: input.topup_credits, payment_method_binding_id: input.payment_method_binding_id }
    : { checkout_kind: input.checkout_kind, payment_method_binding_id: input.payment_method_binding_id }
  return api<CheckoutView>("/checkouts", {
    method: "POST",
    headers: { "idempotency-key": input.idempotencyKey },
    body: JSON.stringify(intent),
  })
}


function refreshAfterCheckout(result: CheckoutView, user: User | null, setUser: (user: User) => void, setCheckout: (checkout: CheckoutView) => void, key: React.MutableRefObject<CheckoutKey | null>, queryClient: ReturnType<typeof useQueryClient>, setError: (error: string) => void) {
  setCheckout(result)
  clearFinishedCheckout(result, user, key)
  if (user) void api<User>("/me").then(setUser).then(() => queryClient.invalidateQueries({ queryKey: ["dashboard"] })).catch(() => undefined)
  setError("")
}

function submitExecution(input: { task_name: string; transaction_id: string }) {
  return api<{ result_text: string }>("/executions", { method: "POST", body: JSON.stringify(input) })
}

function startExecution(user: User, name: string, setPending: (pending: boolean) => void, transaction: React.MutableRefObject<string | null>, mutate: (operation: { task_name: string; transaction_id: string }) => void) {
  let task = name
  if (!transaction.current) task = restoreExecution(user, setPending, transaction) ?? task
  if (!task.trim()) return
  transaction.current ??= crypto.randomUUID()
  setPending(true)
  sessionStorage.setItem(`tasklab_execution_${user.username}`, JSON.stringify({ transaction_id: transaction.current, task_name: task }))
  mutate({ task_name: task, transaction_id: transaction.current })
}

function restoreExecution(user: User, setPending: (pending: boolean) => void, transaction: React.MutableRefObject<string | null>) {
  const saved = sessionStorage.getItem(`tasklab_execution_${user.username}`)
  if (!saved) return null
  const operation = JSON.parse(saved) as { transaction_id: string; task_name: string }
  transaction.current = operation.transaction_id
  setPending(true)
  return operation.task_name
}

function finishExecution(result: { result_text: string }, user: User | null, setResult: (result: string) => void, setTask: (task: string) => void, setPending: (pending: boolean) => void, transaction: React.MutableRefObject<string | null>, queryClient: ReturnType<typeof useQueryClient>, setError: (error: string) => void) {
  setResult(result.result_text)
  setTask("")
  setPending(false)
  transaction.current = null
  if (user) sessionStorage.removeItem(`tasklab_execution_${user.username}`)
  void queryClient.invalidateQueries({ queryKey: ["dashboard"] })
  setError("")
}

async function signOut(user: User, queryClient: ReturnType<typeof useQueryClient>, setUser: (user: null) => void, setCheckout: (checkout: null) => void, setError: (error: string) => void, checkoutKey: React.MutableRefObject<CheckoutKey | null>, executionTransaction: React.MutableRefObject<string | null>) {
  await api("/auth/logout", { method: "POST" }).catch(() => undefined)
  if (checkoutKey.current) sessionStorage.removeItem(`tasklab_checkout_${user.username}_${checkoutKey.current.scope}`)
  sessionStorage.removeItem(`tasklab_execution_${user.username}`)
  checkoutKey.current = null
  executionTransaction.current = null
  setUser(null)
  setCheckout(null)
  setError("")
  queryClient.clear()
}
