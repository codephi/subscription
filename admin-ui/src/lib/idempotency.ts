interface PendingKey {
  transactionId: string;
  key: string;
}

/** Reuse a key for retries of one transaction; e.g. `pendingKey("credit:account", "tx-1")`. */
export function pendingKey(scope: string, transactionId: string): string {
  const storageKey = `subscription-admin:${scope}`;
  const saved = localStorage.getItem(storageKey);
  if (saved) {
    try {
      const previous = JSON.parse(saved) as PendingKey;
      if (previous.transactionId === transactionId && previous.key)
        return previous.key;
    } catch {
      /* A malformed local preference can be replaced safely. */
    }
  }
  const key = crypto.randomUUID();
  localStorage.setItem(
    storageKey,
    JSON.stringify({ transactionId, key } satisfies PendingKey),
  );
  return key;
}

/** Clear a resolved transaction key; e.g. `clearPendingKey("credit:account")`. */
export function clearPendingKey(scope: string): void {
  localStorage.removeItem(`subscription-admin:${scope}`);
}
