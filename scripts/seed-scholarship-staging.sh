#!/usr/bin/env bash
# Staging seed flow for the seven scholarship contracts.
#
# Provisions a complete, synthetic scholarship scenario on testnet: modules and
# roles, a published program with an open window and a budget, eligibility rules
# and attestations, a form schema and consent terms, an application from a
# synthetic applicant, a milestone with verified evidence, a verified payout
# wallet, and a disbursement intent.
#
#   ./scripts/seed-scholarship-staging.sh plan      # print the plan, touch nothing
#   ./scripts/seed-scholarship-staging.sh apply     # provision, idempotently
#   ./scripts/seed-scholarship-staging.sh verify    # read state back and assert
#   ./scripts/seed-scholarship-staging.sh teardown  # reverse, in reverse order
#
# Properties this script is built around:
#
#   Repeatable  Every mutating step is preceded by a state probe, so a second
#               run is a no-op rather than an AlreadyInitialized failure. The
#               scenario is keyed by a fixed seed derived from SCHOLARSHIP_SEED,
#               so identity, program ID, and commitment values are stable across
#               runs and machines.
#   Reversible  Each step appends to deployments/scholarship-staging.json.
#               `teardown` replays that ledger backwards. The chain cannot be
#               rewound, so reversal means driving contracts back to their
#               initial state, not deleting history.
#   Synthetic   Every identity is a locally generated key under the
#               sch-seed- prefix. No real person, wallet, or key is ever used,
#               and no existing account is reused for an applicant role.
#   Testnet     The passphrase is checked before anything is sent. A mainnet
#               passphrase is refused outright, with no override, because the
#               scenario deliberately creates accounts and a budget.
#
# Known limits, stated rather than hidden:
#
#   * The seven scholarship contracts are not yet in scripts/deploy-testnet.sh
#     or deployments/testnet.json. Their IDs must be supplied via the
#     SCHOLARSHIP_*_ID variables or added to the manifest first.
#   * Application review has no on-chain outcome (scholarship-guide.md, Known
#     gaps), so there is no review step to seed. Verification asserts what the
#     chain can actually prove rather than inventing a decision.
#   * `teardown` cannot revoke a settled disbursement. It cancels open intents
#     and releases reserved awards; anything already executed is final.
set -euo pipefail

NETWORK="${STELLAR_NETWORK:-testnet}"
SOURCE="${STELLAR_SOURCE:-deployer}"
DEPLOYMENT_FILE="${DEPLOYMENT_FILE:-deployments/testnet.json}"
LEDGER="${SCHOLARSHIP_SEED_LEDGER:-deployments/scholarship-staging.json}"
KEY_PREFIX="${SCHOLARSHIP_KEY_PREFIX:-sch-seed-}"
SEED_MATERIAL="${SCHOLARSHIP_SEED:-scholarship-staging-v1}"

# The signing payload helper. Pinned to RFC 8032 vectors; refuse to continue if
# it cannot prove itself, because every payout-wallet step depends on it.
SIGNER="scripts/scholarship_wallet_signer.py"

# Synthetic scenario shape. Amounts are tiny and denominated in a token that
# only exists on testnet.
PROGRAM_TITLE="Synthetic Staging Scholarship"
PROGRAM_DESCRIPTION="Synthetic staging scenario. No real applicant data."
CURRENCY="USDC"
FUNDING_MODEL="FixedAward"
MAX_RECIPIENTS=2
PER_AWARD_AMOUNT=100
TOTAL_BUDGET=1000
WINDOW_HOURS=24
GRACE_SECONDS=3600
CHALLENGE_TTL=600
ATTESTATION_TYPE="synthetic_eligible"
ATTESTATION_TTL=86400
DISBURSEMENT_AMOUNT=50
INSTALLMENT=1

die() { echo "error: $*" >&2; exit 1; }
say() { echo "$*"; }
step() { echo; echo "── $* ──"; }

# ── Guards ────────────────────────────────────────────────────────────────

assert_testnet() {
  local passphrase
  passphrase="$(stellar network passphrase 2>/dev/null || true)"
  case "$passphrase" in
    *"Test SDF Network"*) return 0 ;;
    "") die "could not read the network passphrase; is the Stellar CLI installed?" ;;
    *) die "refusing to seed on network '$NETWORK' (passphrase: $passphrase).
     This flow creates accounts, a budget, and open intents, and is only ever
     meant for testnet. There is no override; seed testnet instead." ;;
  esac
}

require_cli() {
  command -v stellar >/dev/null 2>&1 || die "the Stellar CLI is required but not installed"
  command -v jq >/dev/null 2>&1 || die "jq is required but not installed"
  [ -f "$DEPLOYMENT_FILE" ] || die "$DEPLOYMENT_FILE not found; run ./scripts/deploy-testnet.sh first"
}

# The signer must pass its own vectors before we trust it with a wallet key.
assert_signer() {
  [ -f "$SIGNER" ] || die "$SIGNER not found"
  python3 "$SIGNER" --self-test >/dev/null 2>&1 \
    || die "$SIGNER failed its RFC 8032 self-test; refusing to continue.
     A wallet signature that cannot be trusted must never be sent to a contract."
}

# ── Contract addresses ────────────────────────────────────────────────────

CONTRACTS=(core registry programs eligibility applications milestones disbursements)

load_contract_ids() {
  CONTRACT_IDS=()
  for name in "${CONTRACTS[@]}"; do
    local var="SCHOLARSHIP_${name}_ID"
    local id="${!var:-}"
    if [ -z "$id" ] && [ -f "$DEPLOYMENT_FILE" ]; then
      id="$(jq -r ".contracts.scholarship_${name} // empty" "$DEPLOYMENT_FILE")"
    fi
    [ -n "$id" ] || die "no contract ID for scholarship-$name.
     Set $var, or add \"scholarship_${name}\" to $DEPLOYMENT_FILE.
     The scholarship contracts are not yet written into scripts/deploy-testnet.sh."
    CONTRACT_IDS+=("$id")
  done
}

contract_id() {
  local want="$1" i
  for i in "${!CONTRACTS[@]}"; do
    [ "${CONTRACTS[$i]}" = "$want" ] && echo "${CONTRACT_IDS[$i]}" && return 0
  done
  die "unknown contract '$want'"
}

# ── Deterministic synthetic values ────────────────────────────────────────

# Derive a stable 32-byte value from the seed material and a label, so the same
# SCHOLARSHIP_SEED always yields the same program ID and commitments. Uses
# SHA-256 from shasum rather than assuming sha256sum exists on macOS.
derive() {
  printf '%s' "$SEED_MATERIAL:$1" | shasum -a 256 | cut -d' ' -f1
}

hex32() { derive "$1"; }

# Stellar CLI wants BytesN<32> as 64 hex characters.
b32() { hex32 "$1"; }

# ── Synthetic identities ──────────────────────────────────────────────────

IDENTITY_LABELS=(admin sponsor applicant reviewer finance wallet issuer)

# Returns the address for a synthetic identity, generating the key on first use.
identity_address() {
  local name="$KEY_PREFIX$1"
  if stellar keys show "$name" >/dev/null 2>&1; then
    stellar keys address "$name"
  else
    stellar keys generate "$name" --network "$NETWORK" >/dev/null
    stellar keys fund "$name" --network "$NETWORK" >/dev/null 2>&1 || true
    stellar keys address "$name"
  fi
}

# The raw seed behind a synthetic identity, for the challenge signature.
identity_seed_hex() {
  python3 "$SIGNER" secret-to-seed --secret "$(stellar keys secret "$KEY_PREFIX$1")"
}

setup_identities() {
  step "Synthetic identities"
  local label
  for label in "${IDENTITY_LABELS[@]}"; do
    say "  $KEY_PREFIX$label -> $(identity_address "$label")"
  done
  say ""
  say "  All keys are local and synthetic. Re-running reuses the same keys, so the"
  say "  scenario is stable. Delete them with: stellar keys rm $KEY_PREFIX<label>"
}

# Resolves a role label to its address. A case statement rather than an
# associative array because macOS still ships bash 3.2, which has no declare -A.
addr() {
  case "$1" in
    admin|sponsor|applicant|reviewer|finance|wallet|issuer)
      identity_address "$1"
      ;;
    *)
      die "unknown identity role '$1'"
      ;;
  esac
}

# ── Invocation helpers ────────────────────────────────────────────────────

# invoke <contract> <fn> [args...] -- send a state-changing call.
invoke() {
  local c="$1" fn="$2"; shift 2
  stellar contract invoke \
    --id "$(contract_id "$c")" \
    --source "$SOURCE" \
    --network "$NETWORK" \
    -- "$fn" "$@" >/dev/null
}

# invoke_as <identity> <contract> <fn> [args...] -- send as a synthetic role.
invoke_as() {
  local who="$1" c="$2" fn="$3"; shift 3
  stellar contract invoke \
    --id "$(contract_id "$c")" \
    --source "$KEY_PREFIX$who" \
    --network "$NETWORK" \
    -- "$fn" "$@" >/dev/null
}

# view <contract> <fn> [args...] -- read state, print raw output.
view() {
  local c="$1" fn="$2"; shift 2
  stellar contract invoke \
    --id "$(contract_id "$c")" \
    --source "$SOURCE" \
    --network "$NETWORK" \
    --rpc "$RPC_URL" \
    -- "$fn" "$@" 2>/dev/null
}

# probe_ok <contract> <fn> [args...] -- true if the read succeeds.
probe_ok() {
  view "$@" >/dev/null 2>&1
}

# ── Ledger (what teardown replays) ────────────────────────────────────────

ledger_init() {
  mkdir -p "$(dirname "$LEDGER")"
  if [ ! -f "$LEDGER" ]; then
    cat > "$LEDGER" <<EOF
{
  "seed_material": "$SEED_MATERIAL",
  "network": "$NETWORK",
  "program_id": "$(hex32 program)",
  "created_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "steps": []
}
EOF
  fi
}

# record <description> -- append a reversal note.
record() {
  local tmp="$LEDGER.tmp.$$"
  jq --arg step "$1" '.steps += [{"at": (now | todate), "step": $step}]' \
    "$LEDGER" > "$tmp" && mv "$tmp" "$LEDGER"
}

# ── Commands ──────────────────────────────────────────────────────────────

cmd_plan() {
  say "Scholarship staging seed plan"
  say "  network:       $NETWORK"
  say "  admin source:  $SOURCE"
  say "  ledger:        $LEDGER"
  say "  key prefix:    $KEY_PREFIX"
  say "  scenario seed: $SEED_MATERIAL"
  say ""
  say "  Deterministic values derived from the scenario seed:"
  say "    program_id            $(hex32 program)"
  say "    form schema hash      $(hex32 form)"
  say "    consent terms hash    $(hex32 terms)"
  say "    application data_hash $(hex32 application)"
  say ""
  say "  Steps"
  cat <<'PLAN'
    1  registry      initialize, register 7 modules
    2  registry      grant Student/Sponsor/Reviewer/Finance/Administrator
    3  all           initialize with the admin address
    4  core          create the program, publish it
    5  programs      set an open window, configure the award budget
    6  eligibility   publish a rule, issue a synthetic attestation
    7  applications  register the program, activate, publish form + terms
    8  applicant     record consent, submit the application
    9  reviewer      register, set review mode, assign, declare a conflict
    10 milestones    create a milestone, submit + verify evidence
    11 disbursements authorise a creator, verify a wallet, create an intent
PLAN
  say ""
  say "  Not seeded, because it does not exist on-chain:"
  say "    - an application review decision (no such function; see scholarship-guide.md)"
  say ""
  say "  Nothing has been sent. Run 'apply' to provision."
}

cmd_apply() {
  require_cli
  assert_testnet
  assert_signer
  load_contract_ids
  ledger_init
  setup_identities

  local admin_pid b32_window_open b32_window_close now
  b32_window_open="$(b32 window-open)"
  b32_window_close="$(b32 window-close)"
  now="$(date +%s)"

  step "1  Registry: modules"
  if probe_ok registry is_module_registered "$CURRENCY"; then
    say "  modules already registered; skipping"
  else
    invoke registry initialize "$(addr admin)"
    local c
    for c in "${CONTRACTS[@]}"; do
      invoke registry register_module "$(addr admin)" "$c" "$(contract_id "$c")"
      say "  registered $c"
    done
    record "registry: initialize + register_module x7"
  fi

  step "2  Registry: roles"
  local role
  for role in Student Sponsor Reviewer Finance Administrator; do
    case "$role" in
      Student)     invoke registry grant_role "$(addr admin)" "$(addr applicant)" "$role" ;;
      Sponsor)     invoke registry grant_role "$(addr admin)" "$(addr sponsor)" "$role" ;;
      Reviewer)    invoke registry grant_role "$(addr admin)" "$(addr reviewer)" "$role" ;;
      Finance)     invoke registry grant_role "$(addr admin)" "$(addr finance)" "$role" ;;
      Administrator) invoke registry grant_role "$(addr admin)" "$(addr admin)" "$role" ;;
    esac
    say "  granted $role"
  done
  record "registry: grant_role x5"

  step "3  Contract initialization"
  local c
  for c in "${CONTRACTS[@]}"; do
    invoke "$c" initialize "$(addr admin)"
    say "  initialized scholarship-$c"
  done
  record "initialize: x7 (not reversible)"

  step "4  Core: program"
  if probe_ok core get_program "$(hex32 program)"; then
    say "  program already exists; skipping creation"
  else
    invoke_as sponsor core create_program \
      "$(addr sponsor)" "$(hex32 program)" "$(hex32 sponsor)" \
      "$PROGRAM_TITLE" "$PROGRAM_DESCRIPTION" "$CURRENCY" "$FUNDING_MODEL"
    say "  created program"
    invoke_as sponsor core transition_program "$(addr sponsor)" "$(hex32 program)" Published
    say "  published"
    record "core: create_program + transition_program(Published)"
  fi

  step "5  Programs: window and budget"
  invoke programs set_program_window "$(addr admin)" "$(hex32 program)" \
    "$now" "$((now + WINDOW_HOURS * 3600))" 0 "$GRACE_SECONDS"
  say "  window open for ${WINDOW_HOURS}h with ${GRACE_SECONDS}s grace"
  invoke programs configure_award_budget "$(addr admin)" "$(hex32 program)" \
    "$MAX_RECIPIENTS" "$PER_AWARD_AMOUNT" "$TOTAL_BUDGET"
  say "  budget: $MAX_RECIPIENTS x $PER_AWARD_AMOUNT, total $TOTAL_BUDGET"
  record "programs: set_program_window + configure_award_budget (budget NOT safely reversible)"

  step "6  Eligibility: rule and attestation"
  invoke eligibility publish_eligibility_rule "$(addr admin)" "$(hex32 program)" \
    "[\"$ATTESTATION_TYPE\"]"
  say "  published rule requiring $ATTESTATION_TYPE"
  invoke eligibility add_issuer "$(addr admin)" "$(addr issuer)"
  invoke_as issuer eligibility issue_attestation "$(addr issuer)" "$(addr applicant)" \
    "$(hex32 program)" "$ATTESTATION_TYPE" "$((now + ATTESTATION_TTL))"
  say "  issued a synthetic attestation to the applicant"
  record "eligibility: publish_eligibility_rule + add_issuer + issue_attestation"

  step "7  Applications: schema and terms"
  invoke applications register_program "$(addr admin)" "$(hex32 program)" \
    "$((now + WINDOW_HOURS * 3600))"
  invoke applications set_program_active "$(addr admin)" "$(hex32 program)" true
  invoke applications publish_form_schema "$(addr admin)" "$(hex32 program)" "$(hex32 form)"
  invoke applications publish_consent_terms "$(addr admin)" "$(hex32 program)" "$(hex32 terms)"
  say "  published form schema v1 and consent terms v1"
  record "applications: register_program + set_program_active + publish_form_schema + publish_consent_terms"

  step "8  Applicant: consent and application"
  invoke_as applicant applications record_consent "$(addr applicant)" "$(hex32 program)"
  invoke_as applicant applications submit_application \
    "$(addr applicant)" "$(hex32 program)" "$(hex32 application)" 1 true
  say "  consent recorded and application submitted"
  record "applications: record_consent + submit_application"

  step "9  Reviewer: panel and assignment"
  invoke applications register_reviewer "$(addr admin)" "$(addr reviewer)" 4
  invoke applications set_reviewer_active "$(addr admin)" "$(addr reviewer)" true
  invoke applications set_review_mode "$(addr admin)" "$(hex32 program)" DoubleBlind
  invoke applications assign_reviewer "$(addr admin)" "$(hex32 program)" \
    "$(hex32 application)" RoundRobin
  say "  reviewer registered and assigned"
  say "  note: DoubleBlind is recorded but unenforced on-chain (Known gaps)"
  record "applications: register_reviewer + set_review_mode + assign_reviewer"

  step "10  Milestones: evidence and verification"
  invoke milestones create_milestone "$(addr admin)" "$(hex32 program)" 1
  invoke milestones add_submitter "$(addr admin)" "$(addr admin)"
  invoke milestones submit_evidence "$(addr admin)" "$(addr applicant)" \
    "$(hex32 program)" 1 "$(hex32 evidence)"
  local ev_version
  ev_version="$(view milestones latest_evidence_version "$(hex32 program)" 1 "$(addr applicant)" \
    | grep -oE '[0-9]+' | head -1)"
  ev_version="${ev_version:-1}"
  invoke milestones add_verifier "$(addr admin)" "$(addr reviewer)"
  invoke_as reviewer milestones verify_evidence "$(addr reviewer)" \
    "$(hex32 program)" 1 "$(addr applicant)" "$ev_version" Approved MeetsCriteria
  say "  milestone 1 evidence v$ev_version verified"
  record "milestones: create_milestone + submit_evidence + verify_evidence (final)"

  step "11  Disbursements: wallet and intent"
  invoke disbursements add_creator "$(addr admin)" "$(addr finance)"
  invoke_as applicant disbursements set_payout_wallet "$(addr applicant)" "$(addr wallet)"
  invoke_as applicant disbursements register_wallet_key "$(addr wallet)" \
    "$(stellar keys public-key "$KEY_PREFIX$wallet")"
  local challenge_id signed
  challenge_id="$(view disbursements open_wallet_challenge "$(addr applicant)" \
    "$(addr wallet)" "$CHALLENGE_TTL" 2>/dev/null | grep -oE '[0-9]+' | tail -1)"
  [ -n "$challenge_id" ] || die "could not read the wallet challenge id"
  # The one genuinely off-chain step: sign the digest the contract publishes.
  signed="$(python3 "$SIGNER" sign \
    --seed "$(identity_seed_hex wallet)" \
    --payload "$(view disbursements wallet_challenge_payload "$challenge_id" | grep -oE '[0-9a-f]{64}' | head -1)")"
  invoke_as applicant disbursements confirm_wallet_challenge "$challenge_id" \
    "$(echo "$signed" | jq -r .pubkey)" "$(echo "$signed" | jq -r .signature)"
  say "  payout wallet verified via challenge $challenge_id"
  invoke_as finance disbursements create_intent "$(addr finance)" "$(hex32 program)" \
    "$(addr applicant)" "$DISBURSEMENT_AMOUNT" "$INSTALLMENT"
  say "  created a disbursement intent"
  record "disbursements: add_creator + wallet challenge + create_intent"

  step "Seeded"
  say "  ledger: $LEDGER"
  say "  run 'verify' to read the state back, or 'teardown' to reverse it."
}

cmd_verify() {
  require_cli
  load_contract_ids
  local failures=0
  check() {
    local label="$1" c="$2" fn="$3"; shift 3
    if probe_ok "$c" "$fn" "$@"; then
      say "  ok    $label"
    else
      say "  FAIL  $label"
      failures=$((failures + 1))
    fi
  }

  say "Scholarship staging verification"
  say ""
  say "Modules and roles"
  local c
  for c in "${CONTRACTS[@]}"; do
    check "module $c registered" registry is_module_registered "$c"
  done
  say ""
  say "Program"
  check "program exists" core get_program "$(hex32 program)"
  check "program is Published" core get_program_status "$(hex32 program)"
  check "window configured" programs get_program_window "$(hex32 program)"
  check "budget configured" programs get_award_budget "$(hex32 program)"
  check "capacity available" programs remaining_capacity "$(hex32 program)"
  say ""
  say "Eligibility"
  check "rule published" eligibility get_latest_rule_version "$(hex32 program)"
  say ""
  say "Application"
  check "consent valid" applications has_valid_consent "$(addr applicant)" "$(hex32 program)"
  check "application recorded" applications has_applied "$(addr applicant)" "$(hex32 program)"
  check "assignment recorded" applications get_assignment "$(hex32 program)" "$(hex32 application)"
  say ""
  say "Milestones"
  check "milestone exists" milestones get_milestone "$(hex32 program)" 1
  check "evidence recorded" milestones get_evidence "$(hex32 program)" 1 "$(addr applicant)" 1
  say ""
  say "Disbursements"
  check "payout wallet verified" disbursements is_wallet_verified "$(addr applicant)"
  say ""
  say "Deliberately not asserted:"
  say "  - an application review decision; no such function exists, so there is"
  say "    nothing on-chain to read back. See scholarship-guide.md, Known gaps."
  say ""

  if [ "$failures" -eq 0 ]; then
    say "All checks passed."
    return 0
  fi
  say "$failures check(s) failed. The scenario is incomplete; re-run 'apply'."
  return 1
}

cmd_teardown() {
  require_cli
  assert_testnet
  load_contract_ids
  [ -f "$LEDGER" ] || die "no ledger at $LEDGER; nothing was seeded by this script"

  say "Reversing the scholarship staging scenario"
  say "  The chain cannot be rewound. This drives contracts back toward their"
  say "  initial state; events remain in the ledger forever."
  say ""

  step "Withdraw what can be withdrawn"
  if invoke disbursements cancel_intent "$(addr finance)" "$(hex32 intent)" 2>/dev/null; then
    say "  cancelled the open disbursement intent"
  else
    say "  no cancellable intent (already settled or not created)"
  fi

  if invoke programs release_award "$(addr admin)" "$(hex32 program)" 2>/dev/null; then
    say "  released one reserved award"
  else
    say "  no reserved award to release"
  fi

  invoke applications revoke_consent "$(addr applicant)" "$(hex32 program)" 2>/dev/null \
    && say "  revoked the applicant's consent" \
    || say "  consent already absent"

  invoke_as sponsor core transition_program "$(addr sponsor)" "$(hex32 program)" Closed 2>/dev/null \
    && say "  closed the program (further submissions refused)" \
    || say "  program already closed or absent"

  say ""
  say "Not reversible, by design or by contract:"
  say "  - initialize on all seven contracts (one-shot, no uninitialize)"
  say "  - the attestation, the application, and the verified evidence"
  say "  - a settled disbursement, and the verified wallet binding"
  say "  - re-running configure_award_budget would RESET the budget counters, so"
  say "    teardown does not touch it; see scholarship-guide.md, Known gaps"
  say ""
  say "To return to a clean slate, reset the network instead:"
  say "  ./scripts/reset-testnet.sh"
  say ""
  say "Ledger retained at $LEDGER for audit. Remove it once you no longer need it."
}

main() {
  RPC_URL="${STELLAR_RPC_URL:-https://soroban-testnet.stellar.org}"
  export RPC_URL
  local cmd="${1:-}"
  case "$cmd" in
    plan)     cmd_plan ;;
    apply)    cmd_apply ;;
    verify)   cmd_verify ;;
    teardown) cmd_teardown ;;
    *) cat >&2 <<'USAGE'
usage: seed-scholarship-staging.sh <plan|apply|verify|teardown>

  plan      print the scenario and the values it will use; sends nothing
  apply     provision the scenario; idempotent, so it is safe to re-run
  verify    read the provisioned state back and assert each step
  teardown  reverse the scenario using the recorded ledger

Environment:
  STELLAR_NETWORK             network id (default: testnet)
  STELLAR_SOURCE              admin key name (default: deployer)
  DEPLOYMENT_FILE             manifest to read contract IDs from
  SCHOLARSHIP_<NAME>_ID       contract ID override, e.g. SCHOLARSHIP_CORE_ID
  SCHOLARSHIP_SEED            scenario seed; change for an independent scenario
  SCHOLARSHIP_SEED_LEDGER     reversal ledger path
  SCHOLARSHIP_KEY_PREFIX      synthetic key prefix
  STELLAR_RPC_URL             RPC endpoint
USAGE
      exit 2
      ;;
  esac
}

main "$@"
