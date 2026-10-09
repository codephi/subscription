import { useState } from "react"
import { CreditCard, Pencil, Plus, Trash2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import type { PaymentMethod } from "@/lib/api"

type Props = {
  methods: PaymentMethod[]
  busy: boolean
  onAdd: (name?: string) => void
  onRename: (id: string, name?: string) => void
  onRemove: (id: string) => void
}

export function PaymentMethodsCard({ methods, busy, onAdd, onRename, onRemove }: Props) {
  const [newName, setNewName] = useState("")
  const [editing, setEditing] = useState<string | null>(null)
  const [editName, setEditName] = useState("")

  return <Card>
    <CardHeader>
      <CardTitle className="flex items-center gap-2"><CreditCard />Cartões salvos</CardTitle>
      <CardDescription>Gerencie cartões para compras futuras e assinaturas.</CardDescription>
    </CardHeader>
    <CardContent className="flex flex-col gap-3">
      {methods.filter((method) => method.status === "ACTIVE").map((method) => <div key={method.payment_method_binding_id} className="flex flex-wrap items-center gap-2 rounded-md border p-3">
        {editing === method.payment_method_binding_id ? <>
          <Input aria-label="Apelido do cartão" value={editName} onChange={(event) => setEditName(event.target.value)} />
          <Button size="sm" disabled={busy} onClick={() => { onRename(method.payment_method_binding_id, editName || undefined); setEditing(null) }}>Salvar</Button>
          <Button size="sm" variant="outline" onClick={() => setEditing(null)}>Cancelar</Button>
        </> : <>
          <span className="min-w-0 flex-1 truncate text-sm">{method.display_name || "Cartão salvo"}</span>
          <Button size="icon" variant="ghost" aria-label="Renomear cartão" disabled={busy} onClick={() => { setEditing(method.payment_method_binding_id); setEditName(method.display_name ?? "") }}><Pencil /></Button>
          <Button size="icon" variant="ghost" aria-label="Remover cartão" disabled={busy} onClick={() => onRemove(method.payment_method_binding_id)}><Trash2 /></Button>
        </>}
      </div>)}
      <Field>
        <FieldLabel htmlFor="new-payment-method-name">Apelido opcional</FieldLabel>
        <Input id="new-payment-method-name" maxLength={50} value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="Ex.: Cartão pessoal" disabled={busy} />
      </Field>
    </CardContent>
    <CardFooter>
      <Button className="w-full" variant="outline" disabled={busy} onClick={() => onAdd(newName.trim() || undefined)}><Plus data-icon="inline-start" />Cadastrar cartão</Button>
    </CardFooter>
  </Card>
}
