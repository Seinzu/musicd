#!/bin/sh
set -eu

env_files="--env-file .env.example"

if [ -f .env ]; then
  env_files="$env_files --env-file .env"
fi

if [ -f .env.local ]; then
  env_files="$env_files --env-file .env.local"
fi

exec docker compose $env_files "$@"
