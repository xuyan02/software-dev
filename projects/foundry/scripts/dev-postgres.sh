#!/usr/bin/env bash
# Start/stop the user-local PostgreSQL used for Foundry development.
#
# The database was installed under ~/.local/share/foundry because it does not
# require root. It listens only on 127.0.0.1:5432.
#
# Usage:
#   scripts/dev-postgres.sh start|stop|status|url
#
# Connection string:
#   postgres://foundry:foundry@127.0.0.1:5432/foundry

set -euo pipefail

BASE="${FOUNDRY_PG_BASE:-$HOME/.local/share/foundry}"
PREFIX="${FOUNDRY_PG_PREFIX:-$BASE/postgres}"
DATA="${FOUNDRY_PG_DATA:-$BASE/pgdata}"
SOCK="${FOUNDRY_PG_SOCKET:-$BASE/pgsock}"
LOG="${FOUNDRY_PG_LOG:-$BASE/postgres.log}"
PORT="${FOUNDRY_PG_PORT:-5432}"

PGBIN="$(echo "$PREFIX"/usr/lib/postgresql/*/bin)"
if [ ! -x "$PGBIN/pg_ctl" ]; then
    echo "PostgreSQL is not installed under $PREFIX" >&2
    exit 1
fi

start() {
    mkdir -p "$SOCK"
    if "$PGBIN/pg_ctl" -D "$DATA" status >/dev/null 2>&1; then
        echo "PostgreSQL is already running on 127.0.0.1:$PORT"
        return
    fi
    "$PGBIN/pg_ctl" -D "$DATA" -l "$LOG" \
        -o "-p $PORT -k $SOCK -c listen_addresses=127.0.0.1" start
}

stop() {
    if "$PGBIN/pg_ctl" -D "$DATA" status >/dev/null 2>&1; then
        "$PGBIN/pg_ctl" -D "$DATA" stop -m fast
    else
        echo "PostgreSQL is not running"
    fi
}

status() {
    "$PGBIN/pg_ctl" -D "$DATA" status || true
}

case "${1:-start}" in
    start) start ;;
    stop) stop ;;
    status) status ;;
    url) echo "postgres://foundry:foundry@127.0.0.1:$PORT/foundry" ;;
    *)
        echo "usage: $0 start|stop|status|url" >&2
        exit 2
        ;;
esac
