import { Button } from "@/components/ui/button";

interface CursorPagerProps {
  canGoBack: boolean;
  nextCursor?: string | null;
  onBack: () => void;
  onNext: () => void;
}

export function CursorPager({
  canGoBack,
  nextCursor,
  onBack,
  onNext,
}: CursorPagerProps) {
  return (
    <div className="flex gap-2">
      <Button variant="outline" disabled={!canGoBack} onClick={onBack}>
        Anterior
      </Button>
      <Button variant="outline" disabled={!nextCursor} onClick={onNext}>
        Próxima
      </Button>
    </div>
  );
}
