#!/usr/bin/env bash
set -euo pipefail

export GMSOL_TEST=glv_account_limit_follows_validator_configuration
export GMSOL_RNG=64012
export EXTRA_CARGO_ARGS="${EXTRA_CARGO_ARGS:---nocapture}"
export RUST_LOG="${RUST_LOG:-info}"

run_local=false
active_feature=false

usage() {
    echo "Usage: $0 [--local] [--active-feature]" >&2
    echo "  --local           run on an Anchor-managed local validator (default: Devnet)" >&2
    echo "  --active-feature  activate the 128-account feature; requires --local" >&2
    echo "  no flags          run directly against Devnet" >&2
}

while (($# > 0)); do
    case "$1" in
        --local)
            run_local=true
            ;;
        --active-feature)
            active_feature=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage
            exit 2
            ;;
    esac
    shift
done

if [[ "$active_feature" == true && "$run_local" == false ]]; then
    echo "--active-feature requires --local; public Devnet feature state cannot be changed by this script." >&2
    exit 2
fi

devnet_retries="${DEVNET_RETRIES:-3}"
retry_delay_seconds="${DEVNET_RETRY_DELAY_SECONDS:-10}"
anchor_bin="${ANCHOR_BIN:-anchor}"
feature_id="9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
result_dir="${GLV_RESULT_DIR:-target/glv-account-limit/$timestamp}"
summary_file="$result_dir/summary.txt"
lock_dir="target/glv-account-limit/.config-lock"
anchor_toml="Anchor.toml"
anchor_backup=""
lock_acquired=false

mkdir -p "$result_dir" "$(dirname "$lock_dir")"
touch "$summary_file"

cleanup() {
    if [[ -n "$anchor_backup" && -f "$anchor_backup" ]]; then
        cp "$anchor_backup" "$anchor_toml"
        rm -f "$anchor_backup"
    fi
    if [[ "$lock_acquired" == true ]]; then
        rmdir "$lock_dir" 2>/dev/null || true
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

acquire_lock() {
    if ! mkdir "$lock_dir" 2>/dev/null; then
        echo "Another GLV local comparison is changing Anchor.toml: $lock_dir" >&2
        exit 1
    fi
    lock_acquired=true
}

set_feature_state() {
    local active="$1"
    if [[ -n "$anchor_backup" ]]; then
        cp "$anchor_backup" "$anchor_toml"
    else
        anchor_backup="$(mktemp "$result_dir/Anchor.toml.backup.XXXXXX")"
        cp "$anchor_toml" "$anchor_backup"
    fi

    if [[ "$active" == true ]]; then
        if ! grep -Fq "deactivate_feature = [\"$feature_id\"]" "$anchor_backup"; then
            echo "Expected feature deactivation entry was not found in $anchor_toml" >&2
            exit 1
        fi
        local temporary
        temporary="$(mktemp "$result_dir/Anchor.toml.active.XXXXXX")"
        awk -v feature="$feature_id" \
            'index($0, "deactivate_feature") && index($0, feature) { next } { print }' \
            "$anchor_backup" >"$temporary"
        cp "$temporary" "$anchor_toml"
        rm -f "$temporary"
        if grep -E 'deactivate_feature.*'"$feature_id" "$anchor_toml" >/dev/null; then
            echo "Failed to activate feature $feature_id in temporary Anchor config" >&2
            exit 1
        fi
    fi
}

run_local_case() {
    local limit="$1"
    local active="$2"
    local log_file="$result_dir/limit-$limit.log"
    local status=1
    local -a anchor_args=(test)

    if [[ "${ANCHOR_SKIP_BUILD:-false}" == true ]]; then
        anchor_args+=(--skip-build)
    fi

    set_feature_state "$active"
    : >"$log_file"
    {
        echo "=== local account limit: $limit ==="
        echo "case_limit=$limit"
        echo "feature_id=$feature_id"
        echo "feature_expected_active=$active"
    } >>"$summary_file"

    for ((attempt = 1; attempt <= devnet_retries; attempt++)); do
        echo "=== limit=$limit attempt=$attempt/$devnet_retries ===" | tee -a "$log_file"
        set +e
        "$anchor_bin" "${anchor_args[@]}" -- --features mock --features devnet,test-only,migration \
            2>&1 | tee -a "$log_file"
        status="${PIPESTATUS[0]}"
        set -e

        if ((status == 0)); then
            break
        fi
        if ! grep -Eq '503 Service Unavailable|HTTP status server error \(503' "$log_file"; then
            break
        fi
        if ((attempt == devnet_retries)); then
            echo "Devnet RPC remained unavailable after ${devnet_retries} attempts." | tee -a "$log_file" >&2
            break
        fi
        echo "Devnet RPC returned HTTP 503; retrying in ${retry_delay_seconds}s (${attempt}/${devnet_retries})..." | tee -a "$log_file" >&2
        sleep "$retry_delay_seconds"
    done

    cp "$anchor_backup" "$anchor_toml"
    if ((status == 0)); then
        if [[ "$limit" == 64 ]]; then
            grep -Fq 'GLV_RESULT case="route-heavy deposit + close" actual=REJECTED_TOO_MANY_ACCOUNT_LOCKS unique_accounts=65' "$log_file" &&
                grep -Fq 'GLV_RESULT case="route-heavy withdrawal + close" actual=REJECTED_TOO_MANY_ACCOUNT_LOCKS unique_accounts=68' "$log_file" ||
                status=1
        else
            grep -Fq 'GLV_RESULT case="route-heavy deposit + close" actual=EXECUTED unique_accounts=65' "$log_file" &&
                grep -Fq 'GLV_RESULT case="route-heavy withdrawal + close" actual=EXECUTED unique_accounts=68' "$log_file" ||
                status=1
        fi
    fi

    {
        echo "exit_status=$status"
        grep -E 'GLV_(SCENARIO|ACCOUNT_REPORT|RESULT)' "$log_file" || true
        echo
    } >>"$summary_file"

    return "$status"
}

run_devnet_case() {
    local log_file="$result_dir/devnet.log"
    local status=1
    local -a extra_args
    read -r -a extra_args <<<"$EXTRA_CARGO_ARGS"

    {
        echo "=== devnet ==="
        echo "target=devnet"
        echo "feature_state=controlled_by_devnet"
    } >>"$summary_file"

    for ((attempt = 1; attempt <= devnet_retries; attempt++)); do
        echo "=== target=devnet attempt=$attempt/$devnet_retries ===" | tee -a "$log_file"
        set +e
        ANCHOR_PROVIDER_URL="${DEVNET_RPC_URL:-https://api.devnet.solana.com}" \
            ANCHOR_WALLET="${ANCHOR_WALLET:-$HOME/.config/solana/id.json}" \
            cargo test -p gmsol-tests --test anchor --features anchor-test -- \
            "$GMSOL_TEST" "${extra_args[@]}" 2>&1 | tee -a "$log_file"
        status="${PIPESTATUS[0]}"
        set -e

        if ((status == 0)); then
            break
        fi
        if ! grep -Eq '503 Service Unavailable|HTTP status server error \(503' "$log_file"; then
            break
        fi
        if ((attempt == devnet_retries)); then
            echo "Devnet RPC remained unavailable after ${devnet_retries} attempts." | tee -a "$log_file" >&2
            break
        fi
        echo "Devnet RPC returned HTTP 503; retrying in ${retry_delay_seconds}s (${attempt}/${devnet_retries})..." | tee -a "$log_file" >&2
        sleep "$retry_delay_seconds"
    done

    {
        echo "exit_status=$status"
        grep -E 'GLV_(SCENARIO|ACCOUNT_REPORT|RESULT)' "$log_file" || true
        echo
    } >>"$summary_file"
    return "$status"
}

status=0
if [[ "$run_local" == true ]]; then
    acquire_lock
    if [[ "$active_feature" == true ]]; then
        run_local_case 128 true || status=$?
    else
        run_local_case 64 false || status=$?
    fi
else
    run_devnet_case || status=$?
fi

echo "GLV account-limit results: $result_dir"
exit "$status"
