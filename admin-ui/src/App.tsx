import { lazy, Suspense, useEffect } from "react";
import {
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import {
  Activity,
  Boxes,
  CreditCard,
  LayoutDashboard,
  Package,
  ScrollText,
  Inbox,
} from "lucide-react";
import { BrowserRouter, Link, NavLink, Route, Routes } from "react-router-dom";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { AppErrorBoundary } from "@/components/app-error-boundary";
import { ThemeToggle } from "@/components/theme-toggle";
import { logFrontendError } from "@/lib/telemetry";
import { useThemeStore } from "@/store/theme-store";

const OverviewPage = lazy(() =>
  import("@/pages/overview-page").then((module) => ({
    default: module.OverviewPage,
  })),
);
const WorkspaceListPage = lazy(() =>
  import("@/pages/workspace-list-page").then((module) => ({
    default: module.WorkspaceListPage,
  })),
);
const WorkspacePage = lazy(() =>
  import("@/pages/workspace-page").then((module) => ({
    default: module.WorkspacePage,
  })),
);
const BillingListPage = lazy(() =>
  import("@/pages/billing-list-page").then((module) => ({
    default: module.BillingListPage,
  })),
);
const BillingDetailPage = lazy(() =>
  import("@/pages/billing-detail-page").then((module) => ({
    default: module.BillingDetailPage,
  })),
);
const CatalogListPage = lazy(() =>
  import("@/pages/catalog-list-page").then((module) => ({
    default: module.CatalogListPage,
  })),
);
const CatalogDetailPage = lazy(() =>
  import("@/pages/catalog-detail-page").then((module) => ({
    default: module.CatalogDetailPage,
  })),
);
const CatalogCreatePage = lazy(() =>
  import("@/pages/catalog-create-page").then((module) => ({
    default: module.CatalogCreatePage,
  })),
);
const WorkspaceActionsPage = lazy(() =>
  import("@/pages/workspace-actions-page").then((module) => ({
    default: module.WorkspaceActionsPage,
  })),
);
const WorkspaceIntegrationsPage = lazy(() =>
  import("@/pages/workspace-integrations-page").then((module) => ({
    default: module.WorkspaceIntegrationsPage,
  })),
);
const PlanActionsPage = lazy(() =>
  import("@/pages/plan-actions-page").then((module) => ({
    default: module.PlanActionsPage,
  })),
);
const AuditPage = lazy(() =>
  import("@/pages/audit-page").then((module) => ({
    default: module.AuditPage,
  })),
);
const InboxPage = lazy(() =>
  import("@/pages/inbox-page").then((module) => ({
    default: module.InboxPage,
  })),
);

const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error) => logFrontendError("query", error),
  }),
  defaultOptions: { queries: { retry: 1, staleTime: 15_000 } },
});

function Navigation() {
  const links = [
    { to: "/", label: "Visão geral", icon: LayoutDashboard },
    { to: "/workspaces", label: "Workspaces", icon: Boxes },
    { to: "/billing/collections", label: "Billing", icon: CreditCard },
    { to: "/catalog/products", label: "Catálogo", icon: Package },
    { to: "/audit", label: "Auditoria", icon: ScrollText },
    { to: "/inbox", label: "Inbox", icon: Inbox },
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
  const preference = useThemeStore((state) => state.preference);
  const systemTheme = useThemeStore((state) => state.systemTheme);
  const setSystemTheme = useThemeStore((state) => state.setSystemTheme);
  const theme = preference === "system" ? systemTheme : preference;

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const updateSystemTheme = () =>
      setSystemTheme(media.matches ? "dark" : "light");
    updateSystemTheme();
    media.addEventListener("change", updateSystemTheme);
    return () => media.removeEventListener("change", updateSystemTheme);
  }, [setSystemTheme]);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.style.colorScheme = theme;
  }, [theme]);

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
        <div className="mb-5 flex justify-end">
          <ThemeToggle />
        </div>
        <Suspense fallback={<p role="status">Carregando página…</p>}>
          <Routes>
            <Route path="/" element={<OverviewPage />} />
            <Route path="/workspaces" element={<WorkspaceListPage />} />
            <Route
              path="/workspaces/:workspaceId"
              element={<WorkspacePage />}
            />
            <Route
              path="/workspaces/:workspaceId/actions"
              element={<WorkspaceActionsPage />}
            />
            <Route
              path="/workspaces/:workspaceId/integrations"
              element={<WorkspaceIntegrationsPage />}
            />
            <Route
              path="/workspaces/:workspaceId/plans/:planId/actions"
              element={<PlanActionsPage />}
            />
            <Route path="/billing/:kind" element={<BillingListPage />} />
            <Route path="/billing/:kind/:id" element={<BillingDetailPage />} />
            <Route path="/catalog/:kind" element={<CatalogListPage />} />
            <Route path="/catalog/:kind/new" element={<CatalogCreatePage />} />
            <Route path="/catalog/:kind/:id" element={<CatalogDetailPage />} />
            <Route path="/audit" element={<AuditPage />} />
            <Route path="/inbox" element={<InboxPage />} />
            <Route path="*" element={<div>Página não encontrada.</div>} />
          </Routes>
        </Suspense>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <AppErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <Shell />
        </BrowserRouter>
      </QueryClientProvider>
    </AppErrorBoundary>
  );
}
