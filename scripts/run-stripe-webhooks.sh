#!/usr/bin/env bash
set -euo pipefail

if [ -f .env ]; then
  set -a
  . ./.env
  set +a
fi

if [ "${BILLING_SANDBOX_ENABLED:-false}" != "true" ]; then
  echo "Stripe webhook forwarding is disabled."
  while true; do sleep 3600; done
fi

if ! command -v stripe >/dev/null 2>&1; then
  echo "Install Stripe CLI to run the enabled local billing sandbox." >&2
  exit 1
fi

if [ -z "${STRIPE_SECRET_KEY:-}" ] || [ -z "${STRIPE_WEBHOOK_SECRET:-}" ]; then
  echo "Set STRIPE_SECRET_KEY and STRIPE_WEBHOOK_SECRET in the Subscription environment." >&2
  exit 1
fi

export STRIPE_API_KEY="$STRIPE_SECRET_KEY"
listener_secret=$(stripe listen --print-secret)
if [ "$listener_secret" != "$STRIPE_WEBHOOK_SECRET" ]; then
  echo "STRIPE_WEBHOOK_SECRET does not match the Stripe CLI listener secret." >&2
  exit 1
fi

echo "Forwarding Stripe test webhooks to the Subscription."
stripe listen \
  --events payment_intent.succeeded,payment_intent.payment_failed,payment_intent.canceled,payment_intent.requires_action,charge.refunded \
  --forward-to http://localhost:3000/v1/billing/webhooks/stripe \
  2>&1 | sed -E 's/whsec_[A-Za-z0-9]+/[REDACTED]/g'
