#!/usr/bin/env bash
# Exercise the AgentMemory adapter against the pinned upstream service.

set -euo pipefail

compose=(
  docker compose
  -f integration/remote-engines/docker-compose.yml
  --profile agentmemory
)

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    "${compose[@]}" logs agentmemory agentmemory-engine agentmemory-init || true
  fi
  "${compose[@]}" rm -s -f agentmemory agentmemory-engine agentmemory-init || true
  exit "$status"
}
trap cleanup EXIT

"${compose[@]}" up -d --build

for _ in $(seq 1 60); do
  if curl --fail --silent http://127.0.0.1:3111/agentmemory/livez >/dev/null; then
    cargo run -p tinymemory-remote --example conformance -- agentmemory http://127.0.0.1:3111
    exit 0
  fi
  sleep 2
done

echo "AgentMemory did not become ready within 120 seconds." >&2
exit 1
