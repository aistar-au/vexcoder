#!/usr/bin/env bash
set -euo pipefail

PATTERNS=(
    "message_tx"
    "message_rx"
    "send_message("
    "ConversationStreamUpdate"
    "update_rx\.recv"
    "update_rx\.try_recv"
)

RUNTIME_POLICY_PATTERNS=(
    "fn strip_tagged_tool_markup"
    "fn request_likely_requires_tools"
)

FAIL=0
APP_TARGET="src/app.rs"
STATE_TARGET="src/state/"
for pattern in "${PATTERNS[@]}"; do
    if grep -rn "$pattern" "$APP_TARGET"; then
        echo "FAIL: forbidden pattern '$pattern' found in $APP_TARGET"
        FAIL=1
    fi
done

for pattern in "${RUNTIME_POLICY_PATTERNS[@]}"; do
    if grep -rn "$pattern" "$APP_TARGET" "$STATE_TARGET"; then
        echo "FAIL: runtime policy duplicate '$pattern' found outside src/runtime/policy.rs"
        FAIL=1
    fi
done

# ADR-028: CLI binary must not import the transport layer directly.
if [ -f src/bin/vex.rs ]; then
    if grep -n 'vexapi::server' src/bin/vex.rs; then
        echo "FAIL: src/bin/vex.rs imports vexapi::server directly — use crate-root re-export (ADR-028)"
        FAIL=1
    fi
fi

# ADR-028: local_api.rs internal types must be pub(crate), not pub.
if [ -f src/local_api.rs ]; then
    if grep -En '^pub (struct|enum) (LocalApiState|ActiveTask|LocalApiTaskShared|PendingApproval|FrontendCommand|LocalApiFrontend|LocalApiMode) ' src/local_api.rs | grep -v 'pub(crate)'; then
        echo "FAIL: src/local_api.rs internal types must be pub(crate), not pub (ADR-028)"
        FAIL=1
    fi
fi

# ADR-030: persist_current_task_state must not silence errors in release builds.
if [ -f src/app/turn.rs ]; then
    if grep -n 'cfg(debug_assertions)' src/app/turn.rs; then
        echo "FAIL: src/app/turn.rs uses debug_assertions gate — persist errors must be visible in release (ADR-030)"
        FAIL=1
    fi
fi

# ADR-030: Terminal status changes (Completed, Cancelled, MaxTurnsReached, Failed)
# in model_update.rs must use set_task_status() for immediate persist, not direct
# field writes.  Running status is exempt — it is set on the hot streaming path
# where per-delta persistence would be excessive.  turn.rs is exempt because
# commit_completed_turn batches multiple field updates before a single persist.
if [ -f src/app/model_update.rs ]; then
    if grep -n 'self\.current_task\.status = TaskStatus::' src/app/model_update.rs | grep -v 'TaskStatus::Running'; then
        echo "FAIL: src/app/model_update.rs uses direct terminal-status writes — use set_task_status() (ADR-030)"
        FAIL=1
    fi
fi

if [ $FAIL -eq 1 ]; then
    echo ""
    echo "Alternate routing is forbidden. See ADR-007."
    echo "Runtime policy centralization is required. See ADR-014."
    echo "Transport boundary violations are forbidden. See ADR-028."
    echo "State persistence must not be silenced. See ADR-030."
    exit 1
fi

echo "check_no_alternate_routing: clean"
