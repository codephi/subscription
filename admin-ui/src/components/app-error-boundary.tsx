import { Component, type ErrorInfo, type ReactNode } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { logFrontendError } from "@/lib/telemetry";

interface Props {
  children: ReactNode;
}
interface State {
  failed: boolean;
}

export class AppErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, _info: ErrorInfo): void {
    logFrontendError("render", error);
  }

  render(): ReactNode {
    if (this.state.failed)
      return (
        <main className="mx-auto max-w-xl p-8">
          <Alert variant="destructive">
            <AlertTitle>Falha na interface</AlertTitle>
            <AlertDescription>
              Atualize a página. Se o problema continuar, consulte os registros
              do navegador.
            </AlertDescription>
          </Alert>
        </main>
      );
    return this.props.children;
  }
}
