import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useThemeStore } from "@/store/theme-store";

export function ThemeToggle() {
  const theme = useThemeStore((state) => state.theme);
  const toggleTheme = useThemeStore((state) => state.toggleTheme);
  const isDark = theme === "dark";
  const label = isDark ? "Ativar modo claro" : "Ativar modo escuro";
  const Icon = isDark ? Sun : Moon;

  return (
    <Button
      variant="outline"
      className="w-full justify-start"
      aria-label={label}
      aria-pressed={isDark}
      onClick={toggleTheme}
    >
      <Icon data-icon="inline-start" />
      {label}
    </Button>
  );
}
