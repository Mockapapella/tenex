#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(pwd)"
test_name="__repro_docker_git_root"
test_file="$repo_root/tests/${test_name}.rs"

cp "$script_dir/repro_docker_git_root.rs" "$test_file"
cleanup() {
  rm -f "$test_file"
}
trap cleanup EXIT

cargo test --test "$test_name" -- --exact --nocapture
