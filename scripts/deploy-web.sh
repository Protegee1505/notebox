#!/usr/bin/env bash
# Guarded deploy for the Notebox web app and Worker.
#
#   ./scripts/deploy-web.sh --check       # report only, no build or upload
#   ./scripts/deploy-web.sh --if-needed   # deploy only if main has runtime changes
#   ./scripts/deploy-web.sh               # deploy the current tip of origin/main
#
# The deploy refuses unless:
#   - the working tree is clean and HEAD is the tip of origin/main,
#   - the CI run for that commit is green (--wait waits for a running run),
#   - both remote D1 databases have no pending migrations.
# It then builds with NBX_BUILD_ID/NBX_GIT_SHA, uploads, and checks that
# /version.json reports the same commit.
#
# Options:
#   --check       report live vs main, CI, migrations and tools; change nothing
#   --if-needed   skip if live already matches main or only docs/CI/tooling changed
#   --wait        wait for CI instead of failing while it is queued or running
#   --no-fetch    do not run `git fetch origin main` (offline check)
#   --dry-run     run the checks and build, then stop before uploading
#   -h, --help    show this help
#
# Environment:
#   NBX_LIVE_URL      default https://notebox.nbx-7207a015.workers.dev
#   NBX_CI_WORKFLOW   default ci.yml
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# The agent shell is often non-interactive and has no ~/.cargo/bin on PATH,
# even though dx and worker-build were installed there.
if [[ -d "${HOME}/.cargo/bin" ]]; then
  PATH="${HOME}/.cargo/bin:${PATH}"
  export PATH
fi

LIVE_URL="${NBX_LIVE_URL:-https://notebox.nbx-7207a015.workers.dev}"
CI_WORKFLOW="${NBX_CI_WORKFLOW:-ci.yml}"
DX_VERSION="0.7.10"
WORKER_BUILD_VERSION="0.8.7"

mode="deploy"
wait_for_ci=0
do_fetch=1
dry_run=0

head_sha=""
head_short=""
main_sha=""
main_short=""
dirty_list=""
dirty_count=0
fetch_error=""

live_version=""
live_build=""
live_error=""

live_commit=""
declare -a changed=()
change_count=0
runtime_count=0

ci_state="unknown"
ci_conclusion="-"
ci_url="-"
ci_id="-"
ci_error=""

declare -a tool_problems=()
declare -a migration_details=()
declare -a migration_apply=()
declare -a migration_raw=()
migration_ok=1
migration_failed=0

usage() {
  cat <<'EOF'
Guarded deploy for the Notebox web app and Worker.

  ./scripts/deploy-web.sh --check       # report only, no build or upload
  ./scripts/deploy-web.sh --if-needed   # deploy only if main has runtime changes
  ./scripts/deploy-web.sh               # deploy the current tip of origin/main

The deploy refuses unless:
  - the working tree is clean and HEAD is the tip of origin/main,
  - the CI run for that commit is green (--wait waits for a running run),
  - both remote D1 databases have no pending migrations.
It then builds with NBX_BUILD_ID/NBX_GIT_SHA, uploads, and checks that
/version.json reports the same commit.

Options:
  --check       report live vs main, CI, migrations and tools; change nothing
  --if-needed   skip if live already matches main or only docs/CI/tooling changed
  --wait        wait for CI instead of failing while it is queued or running
  --no-fetch    do not run `git fetch origin main` (offline check)
  --dry-run     run the checks and build, then stop before uploading
  -h, --help    show this help

Environment:
  NBX_LIVE_URL      default https://notebox.nbx-7207a015.workers.dev
  NBX_CI_WORKFLOW   default ci.yml
EOF
}

say() { printf '\n==> %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
fail() {
  printf '\nerror: %s\n' "$*" >&2
  exit 1
}

set_mode() {
  if [[ "$mode" != "deploy" && "$mode" != "$1" ]]; then
    fail "--check and --if-needed are mutually exclusive"
  fi
  mode="$1"
}

while (($#)); do
  case "$1" in
    --check) set_mode check ;;
    --if-needed) set_mode if-needed ;;
    --wait) wait_for_ci=1 ;;
    --no-fetch) do_fetch=0 ;;
    --dry-run) dry_run=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'error: unknown option %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

# ---------------------------------------------------------------- checks

collect_git_state() {
  if ((do_fetch)); then
    if ! git fetch --quiet origin main 2>/dev/null; then
      fetch_error="git fetch origin main failed; check the network or use --no-fetch"
    fi
  fi

  head_sha="$(git rev-parse HEAD)"
  head_short="$(git rev-parse --short HEAD)"
  main_sha="$(git rev-parse origin/main)"
  main_short="$(git rev-parse --short origin/main)"
  dirty_list="$(git status --porcelain)"
  if [[ -n "$dirty_list" ]]; then
    dirty_count="$(printf '%s\n' "$dirty_list" | wc -l | tr -d ' ')"
  else
    dirty_count=0
  fi
}

check_toolchain() {
  tool_problems=()

  for tool in git gh curl node npx; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      tool_problems+=("$tool is not on PATH")
    fi
  done

  if command -v dx >/dev/null 2>&1; then
    local dx_found
    dx_found="$(dx --version 2>/dev/null | awk '{print $2}')"
    if [[ "$dx_found" != "$DX_VERSION" ]]; then
      tool_problems+=("dx ${DX_VERSION} required, found ${dx_found:-unknown}; install with: curl -sSL https://dioxus.dev/install.sh | bash -s v${DX_VERSION}")
    fi
  else
    tool_problems+=("dx ${DX_VERSION} is not on PATH; install with: curl -sSL https://dioxus.dev/install.sh | bash -s v${DX_VERSION}")
  fi

  if command -v worker-build >/dev/null 2>&1; then
    local wb_found
    wb_found="$(worker-build --version 2>/dev/null | head -n1 | awk '{print $1}')"
    if [[ "$wb_found" != "$WORKER_BUILD_VERSION" ]]; then
      tool_problems+=("worker-build ${WORKER_BUILD_VERSION} required, found ${wb_found:-unknown}; install with: cargo install worker-build --version ${WORKER_BUILD_VERSION} --locked")
    fi
  else
    tool_problems+=("worker-build ${WORKER_BUILD_VERSION} is not on PATH; install with: cargo install worker-build --version ${WORKER_BUILD_VERSION} --locked")
  fi
}

fetch_live() {
  live_version=""
  live_build=""
  live_error=""

  if ! command -v curl >/dev/null 2>&1 || ! command -v node >/dev/null 2>&1; then
    live_error="curl or node is missing"
    return 1
  fi

  local json
  if ! json="$(curl -fsS --max-time 10 "${LIVE_URL}/version.json" 2>/dev/null)"; then
    live_error="could not read ${LIVE_URL}/version.json"
    return 1
  fi

  local parsed
  if ! parsed="$(node -e '
    const j = JSON.parse(require("fs").readFileSync(0, "utf8"));
    process.stdout.write(`${j.version ?? ""}\t${j.build ?? ""}`);
  ' <<<"$json" 2>/dev/null)"; then
    live_error="${LIVE_URL}/version.json is not valid JSON"
    return 1
  fi

  IFS=$'\t' read -r live_version live_build <<<"$parsed"
  if [[ -z "$live_build" ]]; then
    live_error="${LIVE_URL}/version.json has no build id"
    return 1
  fi
}

compute_diff() {
  changed=()
  change_count=0
  runtime_count=0
  live_commit=""

  if [[ -z "$live_build" ]]; then
    return 1
  fi

  if ! live_commit="$(git rev-parse --verify --quiet "${live_build}^{commit}" 2>/dev/null)"; then
    live_commit=""
    return 1
  fi

  if [[ "$live_commit" == "$head_sha" ]]; then
    return 0
  fi

  mapfile -t changed < <(git diff --name-only "${live_commit}..${head_sha}")
  change_count="${#changed[@]}"

  local path
  for path in "${changed[@]}"; do
    if is_runtime_path "$path"; then
      runtime_count=$((runtime_count + 1))
    fi
  done
}

# Return 0 for a path that can change the deployed web app or Worker.
is_runtime_path() {
  case "$1" in
    docs/*|*.md|.github/*|LICENSE|.gitignore|.gitattributes|.editorconfig)
      return 1
      ;;
    scripts/*)
      case "$1" in
        scripts/web-build.sh|scripts/pwa-build.mjs|scripts/web-budget.mjs)
          return 0
          ;;
        *)
          return 1
          ;;
      esac
      ;;
    *)
      return 0
      ;;
  esac
}

lookup_ci() {
  ci_state="unknown"
  ci_conclusion="-"
  ci_url="-"
  ci_id="-"
  ci_error=""

  if ! command -v gh >/dev/null 2>&1; then
    ci_state="error"
    ci_error="gh is not on PATH"
    return 1
  fi

  local line
  if ! line="$(gh run list \
      --workflow "$CI_WORKFLOW" \
      --commit "$head_sha" \
      --limit 1 \
      --json status,conclusion,url,databaseId \
      --jq 'if length == 0 then "missing\t-\t-\t-" else (.[0] | [.status, (.conclusion // "-"), (.url // "-"), (.databaseId | tostring)] | @tsv) end' \
      2>&1)"; then
    ci_state="error"
    ci_error="$line"
    return 1
  fi

  IFS=$'\t' read -r ci_state ci_conclusion ci_url ci_id <<<"$line"
}

check_migrations() {
  migration_details=()
  migration_apply=()
  migration_raw=()
  migration_ok=1
  migration_failed=0

  if ! command -v npx >/dev/null 2>&1; then
    migration_ok=0
    migration_failed=1
    migration_details+=("could not check: npx is not on PATH")
    return
  fi

  local databases=()
  mapfile -t databases < <(sed -n 's/^database_name = "\([^"]*\)"/\1/p' wrangler.toml)
  if ((${#databases[@]} == 0)); then
    migration_ok=0
    migration_failed=1
    migration_details+=("no d1 database_name entries found in wrangler.toml")
    return
  fi

  local db output
  for db in "${databases[@]}"; do
    if ! output="$(npx --yes wrangler d1 migrations list "$db" --remote 2>&1)"; then
      migration_ok=0
      migration_failed=1
      migration_details+=("${db}: could not read remote state")
      migration_raw+=("$output")
      continue
    fi

    if grep -q "No migrations to apply" <<<"$output"; then
      migration_details+=("${db}: none pending")
    else
      migration_ok=0
      migration_apply+=("$db")
      migration_details+=("${db}: PENDING")
      migration_raw+=("$output")
    fi
  done
}

wait_for_ci_run() {
  local attempt
  for ((attempt = 1; attempt <= 24; attempt++)); do
    lookup_ci || true
    if [[ "$ci_state" != "missing" ]]; then
      return 0
    fi
    sleep 5
  done
  return 1
}

ensure_ci_green() {
  if [[ "$ci_state" == "missing" ]] && ((wait_for_ci)); then
    say "Waiting for a CI run for ${head_short}"
    wait_for_ci_run || true
  fi

  if [[ "$ci_state" == "missing" ]]; then
    fail "No CI run found for ${head_short}. Push or merge first, then rerun (or use --wait)."
  fi
  if [[ "$ci_state" == "error" ]]; then
    fail "Could not read CI status for ${head_short}: ${ci_error}"
  fi

  if [[ "$ci_state" != "completed" ]]; then
    if ((wait_for_ci)); then
      say "CI is ${ci_state}; waiting for ${ci_url}"
      if ! gh run watch "$ci_id" --exit-status --interval 10; then
        lookup_ci || true
        fail "CI did not pass: ${ci_url} (conclusion: ${ci_conclusion})"
      fi
      lookup_ci || true
    else
      fail "CI is ${ci_state}. Wait for ${ci_url} or rerun with --wait."
    fi
  fi

  if [[ "$ci_conclusion" != "success" ]]; then
    fail "CI conclusion is ${ci_conclusion}: ${ci_url}"
  fi
}

ensure_migrations_applied() {
  if ((migration_failed)); then
    printf '\n' >&2
    local detail
    for detail in "${migration_details[@]}"; do
      printf '  %s\n' "$detail" >&2
    done
    fail "Could not read the remote D1 migration state. Check \`npx wrangler whoami\` and the database names."
  fi

  if ((!migration_ok)); then
    local db raw
    for raw in "${migration_raw[@]}"; do
      printf '%s\n' "$raw" >&2
    done
    printf '\n' >&2
    for db in "${migration_apply[@]}"; do
      printf '  npx wrangler d1 migrations apply %s --remote\n' "$db" >&2
    done
    fail "The remote D1 databases have pending migrations. Apply them first, then rerun."
  fi
}

# ---------------------------------------------------------------- report

report() {
  local status="READY to deploy"
  local -a blockers=()
  local -a warnings=()

  if [[ -n "$fetch_error" ]]; then
    blockers+=("$fetch_error")
  fi
  if ((dirty_count > 0)); then
    blockers+=("working tree has ${dirty_count} uncommitted change(s)")
  fi
  if [[ "$head_sha" != "$main_sha" ]]; then
    blockers+=("HEAD ${head_short} is not origin/main ${main_short}")
  fi
  if ((${#tool_problems[@]} > 0)); then
    blockers+=("toolchain is incomplete")
  fi
  if [[ "$ci_state" == "error" ]]; then
    blockers+=("CI status could not be read")
  elif [[ "$ci_state" == "missing" ]]; then
    blockers+=("no CI run for ${head_short}")
  elif [[ "$ci_state" != "completed" ]]; then
    blockers+=("CI is ${ci_state}")
  elif [[ "$ci_conclusion" != "success" ]]; then
    blockers+=("CI concluded ${ci_conclusion}")
  fi
  if ((migration_failed)); then
    blockers+=("remote migration state could not be read")
  elif ((!migration_ok)); then
    blockers+=("remote D1 has pending migrations")
  fi

  if [[ -n "$live_error" ]]; then
    warnings+=("$live_error")
  elif [[ "$live_commit" == "$head_sha" ]]; then
    status="Nothing to deploy: live matches ${head_short}"
  elif [[ -n "$live_commit" && "$runtime_count" == 0 ]]; then
    status="Nothing to deploy: ${change_count} file(s) changed, none runtime"
  fi

  if ((${#blockers[@]} > 0)); then
    status="NOT READY"
  fi

  echo "Notebox deploy check"
  echo "  url      ${LIVE_URL}"
  if [[ -n "$live_error" ]]; then
    printf '  live     unknown (%s)\n' "$live_error"
  else
    printf '  live     %s / %s\n' "${live_version:-?}" "${live_build:-?}"
  fi
  printf '  main     %s (%s)\n' "$head_short" "$([[ "$head_sha" == "$main_sha" ]] && echo at-tip || echo not-at-tip)"
  if [[ -n "$live_commit" ]]; then
    if [[ "$live_commit" == "$head_sha" ]]; then
      printf '  diff     none since live\n'
    else
      printf '  diff     %s file(s), %s runtime, since %s\n' "$change_count" "$runtime_count" "$live_build"
      local path
      local shown=0
      for path in "${changed[@]}"; do
        if ((shown >= 12)); then
          break
        fi
        printf '           %s\n' "$path"
        shown=$((shown + 1))
      done
      if ((change_count > shown)); then
        printf '           ... and %s more\n' "$((change_count - shown))"
      fi
    fi
  elif [[ -n "$live_build" ]]; then
    printf '  diff     unknown base %s\n' "$live_build"
  fi
  case "$ci_state" in
    completed) printf '  ci       %s (%s)\n' "$ci_conclusion" "$ci_url" ;;
    missing) printf '  ci       no run for %s\n' "$head_short" ;;
    error) printf '  ci       error: %s\n' "$ci_error" ;;
    *) printf '  ci       %s (%s)\n' "$ci_state" "$ci_url" ;;
  esac
  if ((${#migration_details[@]} > 0)); then
    printf '  d1       %s\n' "${migration_details[0]}"
    local detail
    for detail in "${migration_details[@]:1}"; do
      printf '           %s\n' "$detail"
    done
  fi
  if ((${#tool_problems[@]} > 0)); then
    local problem
    for problem in "${tool_problems[@]}"; do
      printf '  tools    %s\n' "$problem"
    done
  else
    printf '  tools    dx %s, worker-build %s\n' "$DX_VERSION" "$WORKER_BUILD_VERSION"
  fi

  if ((${#warnings[@]} > 0)); then
    local warning
    for warning in "${warnings[@]}"; do
      printf '  warning  %s\n' "$warning"
    done
  fi
  if ((${#blockers[@]} > 0)); then
    local blocker
    for blocker in "${blockers[@]}"; do
      printf '  blocked  %s\n' "$blocker"
    done
  fi
  printf '  result   %s\n' "$status"

  if ((${#migration_raw[@]} > 0)); then
    echo
    echo "Pending migration output:"
    local raw
    for raw in "${migration_raw[@]}"; do
      printf '%s\n' "$raw"
    done
  fi

  ((${#blockers[@]} == 0))
}

# ---------------------------------------------------------------- deploy

build_web() {
  say "Building web release for ${head_short}"
  # wrangler.toml points [assets] at target/dx/..., so force the build there
  # even if the caller exported a different CARGO_TARGET_DIR.
  CARGO_TARGET_DIR="$root/target" \
    NBX_BUILD_ID="$head_short" \
    NBX_GIT_SHA="$head_sha" \
    ./scripts/web-build.sh

  say "Checking the brotli first-load budget"
  node scripts/web-budget.mjs "$root/target/dx/notebox/release/web/public"
}

deploy_worker() {
  say "Deploying ${head_short} to Cloudflare"
  npx --yes wrangler deploy --message "notebox ${head_short}"
}

verify_live() {
  say "Verifying ${LIVE_URL}/version.json"
  local attempt parsed version build
  for ((attempt = 1; attempt <= 12; attempt++)); do
    if parsed="$(curl -fsS --max-time 10 "${LIVE_URL}/version.json" 2>/dev/null \
      | node -e '
          const j = JSON.parse(require("fs").readFileSync(0, "utf8"));
          process.stdout.write(`${j.version ?? ""}\t${j.build ?? ""}`);
        ' 2>/dev/null)"; then
      IFS=$'\t' read -r version build <<<"$parsed"
      if [[ "$build" == "$head_short" ]]; then
        live_version="$version"
        live_build="$build"
        note "live build is ${build}"
        return 0
      fi
      note "still ${build:-unknown}; retrying in 5s"
    else
      note "not reachable yet; retrying in 5s"
    fi
    sleep 5
  done
  return 1
}

collect_git_state
check_toolchain
fetch_live || true
compute_diff || true
lookup_ci || true
check_migrations

if [[ "$mode" == "check" ]]; then
  if report; then
    exit 0
  else
    exit 1
  fi
fi

if [[ -n "$fetch_error" ]]; then
  fail "$fetch_error"
fi
if ((dirty_count > 0)); then
  fail "Working tree has ${dirty_count} uncommitted change(s). Commit or stash them before deploying."
fi
if [[ "$head_sha" != "$main_sha" ]]; then
  fail "HEAD ${head_short} is not the tip of origin/main ${main_short}. Merge first, then run from a clean main checkout."
fi
if ((${#tool_problems[@]} > 0)); then
  fail "${tool_problems[0]} (see --check for the full list)"
fi

if [[ "$mode" == "if-needed" ]]; then
  if [[ -n "$live_error" ]]; then
    fail "Could not read the live build: ${live_error}. Run without --if-needed to force a deploy."
  fi
  if [[ "$live_commit" == "$head_sha" ]]; then
    say "Live already matches ${head_short}; nothing to deploy."
    exit 0
  fi
  if [[ -z "$live_commit" ]]; then
    fail "The live build ${live_build} is not a commit in this checkout. Run without --if-needed to force a deploy."
  fi
  if ((runtime_count == 0)); then
    say "Only docs, CI or tooling changed since ${live_build}; nothing to deploy."
    exit 0
  fi
fi

ensure_ci_green
ensure_migrations_applied

if ((dry_run)); then
  build_web
  say "Dry run: build passed; not uploading."
  exit 0
fi

build_web
deploy_worker

if ! verify_live; then
  fail "The upload finished but ${LIVE_URL}/version.json does not report ${head_short}. Check \`npx wrangler deployments list\` and roll back with \`npx wrangler rollback\`."
fi

say "Deployed"
note "url:      ${LIVE_URL}"
note "version:  ${live_version:-?}"
note "build:    ${head_short} (${head_sha})"
note "rollback: npx wrangler rollback"
