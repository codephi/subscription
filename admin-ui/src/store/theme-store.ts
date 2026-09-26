import { create } from "zustand";

export type ColorTheme = "light" | "dark";

const themeStorageKey = "subscription-admin-theme";

function readStoredTheme(): ColorTheme {
  if (typeof window === "undefined") return "light";
  return window.localStorage.getItem(themeStorageKey) === "dark"
    ? "dark"
    : "light";
}

function saveTheme(theme: ColorTheme): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(themeStorageKey, theme);
}

interface ThemeState {
  theme: ColorTheme;
  toggleTheme: () => void;
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: readStoredTheme(),
  toggleTheme: () => {
    const theme = get().theme === "dark" ? "light" : "dark";
    saveTheme(theme);
    set({ theme });
  },
}));
