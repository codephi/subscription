import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useThemeStore } from "@/store/theme-store";

export function ThemeToggle() {
  const preference = useThemeStore((state) => state.preference);
  const systemTheme = useThemeStore((state) => state.systemTheme);
  const setPreference = useThemeStore((state) => state.setPreference);
  const theme = preference === "system" ? systemTheme : preference;
  const isDark = theme === "dark";
  const label = isDark ? "Ativar modo claro" : "Ativar modo escuro";
  const Icon = isDark ? Sun : Moon;

  return (
    <Button
      variant="outline"
      size="icon-sm"
      aria-label={label}
      aria-pressed={isDark}
      title={label}
      onClick={() => setPreference(isDark ? "light" : "dark")}
    >
      <Icon aria-hidden="true" />
    </Button>
  );
}
