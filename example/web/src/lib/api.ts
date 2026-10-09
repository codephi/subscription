export type User = { username: string; plan_model: "PREPAID" | "SUBSCRIPTION" | null; customer_plan_id: string | null; account_id: string }
export type PaymentMethod = { payment_method_binding_id: string; payment_method: string; display_name: string | null; status: string; created_at: string }
export type Dashboard = { account: User; catalog: { plans: Array<{ plan_version_id: string; price_amount_minor: number; credit_units: number }>; topup_price_per_credit_minor: number; task_cost: number }; customer_plan?: { plan_version_id: string; commercial_status: string; renewal_status: string; cancel_at_period_end: boolean } | null; payment_methods: PaymentMethod[]; wallet_statement: { items: Array<{ signed_credit_units: string; balance_after_credit_units: string; entry_type: string; description: string | null; created_at: string }> }; eligibility: { access_allowed: boolean; balance_credit_units: string | null } | null; meter: { next_block_credit_units: string } | null; checkouts: Array<{ checkout_id: string; checkout_kind: string; status: string; amount_minor: number | null; currency: string | null; granted_credit_units: number | null; created_at: string }>; executions: Array<{ execution_id: string; task_name: string; result_text: string | null; credits_debited: string; status: string; created_at: string }> }

export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const response = await fetch(`/api${path}`, { ...options, credentials: "include", headers: { "content-type": "application/json", ...options.headers } })
  const body = response.status === 204 ? null : await response.json()
  if (!response.ok) throw new Error(body?.message ?? `Falha na solicitação (${response.status})`)
  return body as T
}
