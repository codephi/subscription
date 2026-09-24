import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Activity, Boxes, LayoutDashboard } from "lucide-react";
import { BrowserRouter, Link, NavLink, Route, Routes } from "react-router-dom";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { OverviewPage } from "@/pages/overview-page";
import { WorkspaceListPage } from "@/pages/workspace-list-page";
import { WorkspacePage } from "@/pages/workspace-page";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, staleTime: 15_000 } },
});

function Navigation() {
  const links = [
    { to: "/", label: "Visão geral", icon: LayoutDashboard },
    { to: "/workspaces", label: "Workspaces", icon: Boxes },
  ];
  return (
    <nav aria-label="Navegação principal" className="flex flex-col gap-1">
      {links.map(({ to, label, icon: Icon }) => (
        <NavLink
          key={to}
          to={to}
          end
          className={({ isActive }) =>
            `flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium ${isActive ? "bg-secondary text-secondary-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground"}`
          }
        >
          <Icon aria-hidden="true" className="size-4" />
          {label}
        </NavLink>
      ))}
    </nav>
  );
}

function Shell() {
  return (
    <div className="min-h-screen bg-background text-foreground lg:grid lg:grid-cols-[240px_1fr]">
      <aside className="border-b bg-card p-5 lg:sticky lg:top-0 lg:h-screen lg:border-r lg:border-b-0">
        <Link to="/" className="flex items-center gap-3 text-sm font-semibold">
          <span className="flex size-9 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Activity className="size-5" />
          </span>
          <span>
            Subscription
            <br />
            <span className="text-xs font-normal text-muted-foreground">
              Administração
            </span>
          </span>
        </Link>
        <Separator className="my-5" />
        <Navigation />
        <div className="mt-6">
          <Badge variant="outline">Ambiente interno</Badge>
        </div>
      </aside>
      <main className="mx-auto w-full max-w-7xl p-5 md:p-8 lg:p-10">
        <Routes>
          <Route path="/" element={<OverviewPage />} />
          <Route path="/workspaces" element={<WorkspaceListPage />} />
          <Route path="/workspaces/:workspaceId" element={<WorkspacePage />} />
          <Route path="*" element={<div>Página não encontrada.</div>} />
        </Routes>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Shell />
      </BrowserRouter>
    </QueryClientProvider>
  );
}
