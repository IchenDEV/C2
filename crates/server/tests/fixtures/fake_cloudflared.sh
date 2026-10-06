#!/usr/bin/env sh
# Fake cloudflared for supervisor tests. Never invoke the real binary.
set -eu

dump="${FAKE_CLOUDFLARED_DUMP:-}"
token="${TUNNEL_TOKEN:-}"

# One line per spawn so tests can count and probe live children.
if [ -n "${FAKE_CLOUDFLARED_PIDFILE:-}" ]; then
  echo "$$" >>"$FAKE_CLOUDFLARED_PIDFILE"
fi

if [ -n "$dump" ]; then
  {
    printf 'argv:'
    for arg in "$@"; do printf ' %s' "$arg"; done
    printf '\n'
    printf 'env_hash:%s\n' "$(env | LC_ALL=C sort | sha256sum 2>/dev/null | awk '{print $1}' || env | LC_ALL=C sort | shasum -a 256 | awk '{print $1}')"
    if [ -n "$token" ]; then
      printf 'has_tunnel_token:yes\n'
    else
      printf 'has_tunnel_token:no\n'
    fi
  } >"$dump"
fi

mode="${FAKE_CLOUDFLARED_MODE:-run}"

case "$mode" in
  run)
    if [ -n "$token" ]; then
      echo "connector token echo (should be redacted): $token"
    fi
    if [ -n "${FAKE_CLOUDFLARED_STDOUT:-}${FAKE_CLOUDFLARED_STDERR:-}" ]; then
      # Scripted output (optionally delayed) replaces the default quick-tunnel announcement, so a
      # test can pick the stream and the exact lines a real cloudflared might print.
      sleep "${FAKE_CLOUDFLARED_DELAY:-0}"
      [ -z "${FAKE_CLOUDFLARED_STDOUT:-}" ] || printf '%s\n' "$FAKE_CLOUDFLARED_STDOUT"
      [ -z "${FAKE_CLOUDFLARED_STDERR:-}" ] || printf '%s\n' "$FAKE_CLOUDFLARED_STDERR" >&2
    else
      url_next=0
      for arg in "$@"; do
        if [ "$url_next" = 1 ]; then
          echo "INF Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):"
          echo "https://fake-quick.trycloudflare.com"
          url_next=0
        fi
        if [ "$arg" = "--url" ]; then
          url_next=1
        fi
      done
    fi
    ;;
  exit_immediately)
    exit 3
    ;;
  hang)
    exec sleep 3600
    ;;
  *)
    echo "unknown FAKE_CLOUDFLARED_MODE=$mode" >&2
    exit 2
    ;;
esac

if [ -n "${FAKE_CLOUDFLARED_EXIT_AFTER:-}" ]; then
  sleep "${FAKE_CLOUDFLARED_EXIT_AFTER}"
  exit 4
fi

# exec: the pid recorded above stays the live process, so a kill leaves no orphan.
exec sleep 3600
