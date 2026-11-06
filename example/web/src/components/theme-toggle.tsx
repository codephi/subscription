import { Moon, Sun } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useThemeStore } from "@/store/theme-store"

export function ThemeToggle() {
  const preference = useThemeStore((state) => state.preference)
  const systemTheme = useThemeStore((state) => state.systemTheme)
  const setPreference = useThemeStore((state) => state.setPreference)
  const theme = preference === "system" ? systemTheme : preference
  const dark = theme === "dark"

  const label = dark ? "Ativar tema claro" : "Ativar tema escuro"
  return <Button
    aria-label={label}
    aria-pressed={dark}
    title={label}
    variant="outline"
    size="icon-sm"
    className="fixed right-4 top-4 z-50 rounded-full bg-background shadow-sm"
    onClick={() => setPreference(dark ? "light" : "dark")}
  >
    {dark ? <Sun /> : <Moon />}
  </Button>
}
