import { useState, type FormEvent } from "react"
import { ArrowRight, KeyRound, UserPlus } from "lucide-react"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

type AuthMode = "login" | "register"
type Credentials = { username: string; password: string }

type AuthScreenProps = {
  busy: boolean
  error: string
  onErrorClear: () => void
  onSubmit: (mode: AuthMode, credentials: Credentials) => void
}

export function AuthScreen({ busy, error, onErrorClear, onSubmit }: AuthScreenProps) {
  const [mode, setMode] = useState<AuthMode>("login")

  function changeMode(value: string) {
    setMode(value as AuthMode)
    onErrorClear()
  }

  return <main className="auth-shell">
    <Card className="auth-card">
      <CardHeader className="gap-5">
        <Brand />
        <div className="space-y-2">
          <p className="eyebrow">Simulador de produto</p>
          <CardTitle className="text-3xl tracking-tight">Créditos simples. Testes reais.</CardTitle>
          <CardDescription>Uma POC enxuta para experimentar consumo, recargas e assinatura com a Subscription.</CardDescription>
        </div>
      </CardHeader>
      <CardContent>
        <Tabs value={mode} onValueChange={changeMode}>
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="login">Entrar</TabsTrigger>
            <TabsTrigger value="register">Criar conta</TabsTrigger>
          </TabsList>
          <TabsContent value="login"><CredentialsForm mode="login" busy={busy} onSubmit={onSubmit} /></TabsContent>
          <TabsContent value="register"><CredentialsForm mode="register" busy={busy} onSubmit={onSubmit} /></TabsContent>
        </Tabs>
        {error && <Alert variant="destructive" className="mt-5">
          <KeyRound />
          <AlertTitle>Não foi possível acessar</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>}
        <Separator className="my-5" />
        <p className="text-center text-sm text-muted-foreground">Conta de demonstração: <span className="font-medium text-foreground">admin / admin</span></p>
      </CardContent>
    </Card>
  </main>
}

function Brand() {
  return <div className="brand-lockup">
    <span className="brand-mark">T</span>
    <span>TaskLab<span className="text-primary">.</span></span>
  </div>
}

function CredentialsForm({ mode, busy, onSubmit }: { mode: AuthMode; busy: boolean; onSubmit: AuthScreenProps["onSubmit"] }) {
  const [username, setUsername] = useState(mode === "login" ? "admin" : "")
  const [password, setPassword] = useState(mode === "login" ? "admin" : "")

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    onSubmit(mode, { username, password })
  }

  return <form className="auth-form" onSubmit={submit}>
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor={`${mode}-username`}>Usuário</FieldLabel>
        <Input id={`${mode}-username`} value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" required />
      </Field>
      <Field>
        <FieldLabel htmlFor={`${mode}-password`}>Senha</FieldLabel>
        <Input id={`${mode}-password`} type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete={mode === "login" ? "current-password" : "new-password"} required />
      </Field>
    </FieldGroup>
    <Button className="w-full" type="submit" disabled={busy}>
      {mode === "login" ? "Entrar" : "Criar conta"}
      {mode === "login" ? <ArrowRight data-icon="inline-end" /> : <UserPlus data-icon="inline-end" />}
    </Button>
  </form>
}
