import { create } from "zustand"

export type ColorTheme = "light" | "dark"

const themeStorageKey = "tasklab-theme"

function readTheme(): ColorTheme {
  if (typeof window === "undefined") return "light"
  try {
    return window.localStorage.getItem(themeStorageKey) === "dark" ? "dark" : "light"
  } catch {
    return "light"
  }
}

interface ThemeState {
  theme: ColorTheme
  toggleTheme: () => void
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: readTheme(),
  toggleTheme: () => set((state) => {
    const theme = state.theme === "dark" ? "light" : "dark"
    try {
      window.localStorage.setItem(themeStorageKey, theme)
    } catch {
      // Keep the in-memory choice when browser storage is unavailable.
    }
    return { theme }
  }),
}))
