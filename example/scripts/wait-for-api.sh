#!/usr/bin/env bash
set -euo pipefail

attempt=0
until curl --silent --output /dev/null --max-time 1 http://127.0.0.1:3001/api/me; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 60 ]; then
    echo "TaskLab API did not become ready at http://127.0.0.1:3001/api/me" >&2
    exit 1
  fi
  sleep 0.5
done
