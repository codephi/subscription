import { create } from "zustand";

export type ColorTheme = "light" | "dark";
export type ThemePreference = ColorTheme | "system";

const themeStorageKey = "subscription-admin-theme";

function readPreference(): ThemePreference {
  if (typeof window === "undefined") return "system";
  try {
    const preference = window.localStorage.getItem(themeStorageKey);
    return preference === "light" || preference === "dark"
      ? preference
      : "system";
  } catch {
    return "system";
  }
}

function readSystemTheme(): ColorTheme {
  if (typeof window === "undefined" || !window.matchMedia) return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function savePreference(preference: ColorTheme): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(themeStorageKey, preference);
  } catch {
    /* Keep the in-memory choice when browser storage is unavailable. */
  }
}

interface ThemeState {
  preference: ThemePreference;
  systemTheme: ColorTheme;
  setPreference: (preference: ColorTheme) => void;
  setSystemTheme: (theme: ColorTheme) => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  preference: readPreference(),
  systemTheme: readSystemTheme(),
  setPreference: (preference) => {
    savePreference(preference);
    set({ preference });
  },
  setSystemTheme: (systemTheme) => set({ systemTheme }),
}));
