import type { PropsWithChildren } from "react"
import { ThemeToggle } from "@/components/theme-toggle"

export function AppFrame({ children }: PropsWithChildren) {
  return <>
    <ThemeToggle />
    {children}
  </>
}
