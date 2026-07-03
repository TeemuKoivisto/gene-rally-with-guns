#!/usr/bin/env bash

if [ -f .env ]; then
  set -a
  . ./.env
  set +a
fi

# Make sure cargo is available even in shells that haven't sourced rustup's env.
export PATH="$HOME/.cargo/bin:$PATH"

# Cap build parallelism so the machine stays responsive (override: JOBS=8 ./ex.sh build).
JOBS="${JOBS:-4}"

case "$1" in
run)
  cargo run -j "$JOBS"
  ;;
run:release)
  cargo run --release -j "$JOBS"
  ;;
build)
  nice -n 15 cargo build -j "$JOBS"
  ;;
check)
  cargo check -j "$JOBS"
  ;;
*)
  echo "Usage: $0 {run|run:release|build|check|db:backup}"
  exit 1
  ;;
esac
