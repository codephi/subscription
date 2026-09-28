import { Moon, Sun } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useThemeStore } from "@/store/theme-store"

export function ThemeToggle() {
  const theme = useThemeStore((state) => state.theme)
  const toggleTheme = useThemeStore((state) => state.toggleTheme)
  const dark = theme === "dark"

  const label = dark ? "Ativar tema claro" : "Ativar tema escuro"
  return <Button
    aria-label={label}
    aria-pressed={dark}
    title={label}
    variant="outline"
    size="icon-sm"
    className="fixed right-4 top-4 z-50 rounded-full bg-background shadow-sm"
    onClick={toggleTheme}
  >
    {dark ? <Sun /> : <Moon />}
  </Button>
}
