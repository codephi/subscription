import "@fontsource-variable/geist"
import "./style.css"
import React from "react"
import { useEffect } from "react"
import ReactDOM from "react-dom/client"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { BrowserRouter, Route, Routes } from "react-router-dom"
import App from "./pages/app"
import { useThemeStore } from "@/store/theme-store"

const queryClient = new QueryClient()

function ThemeSync() {
  const preference = useThemeStore((state) => state.preference)
  const systemTheme = useThemeStore((state) => state.systemTheme)
  const setSystemTheme = useThemeStore((state) => state.setSystemTheme)
  const theme = preference === "system" ? systemTheme : preference

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)")
    const updateSystemTheme = () => setSystemTheme(media.matches ? "dark" : "light")
    updateSystemTheme()
    media.addEventListener("change", updateSystemTheme)
    return () => media.removeEventListener("change", updateSystemTheme)
  }, [setSystemTheme])

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark")
    document.documentElement.style.colorScheme = theme
  }, [theme])

  return null
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode><QueryClientProvider client={queryClient}><BrowserRouter><ThemeSync /><Routes><Route path="*" element={<App />} /></Routes></BrowserRouter></QueryClientProvider></React.StrictMode>,
)
