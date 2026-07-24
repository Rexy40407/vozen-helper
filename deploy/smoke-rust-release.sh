#!/usr/bin/env bash
set -euo pipefail

root=/home/vozen/vozen-helper-rust
set -a
. "$root/shared/.env"
set +a
export HELPER_API_ONLY=true
export HELPER_BIND_ADDR=127.0.0.1:8789
log=/tmp/vozen-helper-rust-smoke.log
"$root/current/bin/vozen-helper" serve >"$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true' EXIT
for _ in $(seq 1 20); do
  if curl --fail --silent http://127.0.0.1:8789/health; then
    printf '\n'
    curl --silent --output /dev/null --write-out 'unauthenticated_api_status=%{http_code}\n' http://127.0.0.1:8789/api/me
    exit 0
  fi
  sleep 0.25
done
cat "$log"
exit 1
