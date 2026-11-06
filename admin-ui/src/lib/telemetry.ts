/** Emit structured frontend diagnostics; e.g. `logFrontendError("query", error)`. */
export function logFrontendError(stage: string, error: unknown): void {
  const issue = error instanceof Error ? error : new Error(String(error));
  console.error(
    JSON.stringify({
      level: "error",
      component: "subscription-admin-ui",
      stage,
      name: issue.name,
      message: issue.message,
      occurred_at: new Date().toISOString(),
    }),
  );
}
