import { useEffect, useState } from "react"
import { Moon, Sun } from "lucide-react"
import { Button } from "@/components/ui/button"

const THEME_STORAGE_KEY = "tasklab-theme"

export function ThemeToggle() {
  const [dark, setDark] = useState(readSavedTheme)

  useEffect(() => applyTheme(dark), [dark])

  const label = dark ? "Ativar tema claro" : "Ativar tema escuro"
  return <Button
    aria-label={label}
    aria-pressed={dark}
    title={label}
    variant="outline"
    size="icon-sm"
    className="fixed right-4 top-4 z-50 rounded-full bg-background shadow-sm"
    onClick={() => setDark((current) => !current)}
  >
    {dark ? <Sun /> : <Moon />}
  </Button>
}

function readSavedTheme() {
  const savedTheme = window.localStorage.getItem(THEME_STORAGE_KEY)
  return savedTheme === "dark"
}

function applyTheme(dark: boolean) {
  document.documentElement.classList.toggle("dark", dark)
  window.localStorage.setItem(THEME_STORAGE_KEY, dark ? "dark" : "light")
}
