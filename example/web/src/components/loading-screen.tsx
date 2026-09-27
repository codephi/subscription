import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"

export function LoadingScreen() {
  return <main className="auth-shell">
    <Card className="auth-card">
      <CardHeader className="space-y-4">
        <div className="brand-lockup"><span className="brand-mark">T</span><span>TaskLab<span className="text-primary">.</span></span></div>
        <div className="flex items-center gap-2 text-sm text-muted-foreground"><Spinner />Abrindo seu espaço…</div>
      </CardHeader>
      <CardContent className="space-y-3">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-9 w-full" />
      </CardContent>
    </Card>
  </main>
}
