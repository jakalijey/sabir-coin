#!/usr/bin/env bash
# End-to-end check on testnet: one entrant skips, one waits, then a transfer and an exit.
set -u
NEAR=${NEAR:-near}
C=${CONTRACT:-sabir-gate.testnet}
A=${ALICE:-sabir-alice.testnet}
B=${BOB:-sabir-bob.testnet}
call() { $NEAR contract call-function as-transaction "$C" "$2" json-args "$3" prepaid-gas '100.0 Tgas' attached-deposit "$4" sign-as "$1" network-config testnet sign-with-legacy-keychain send 2>&1 | grep -E "succeeded|failed|Error|panicked|Smart contract" | head -3; }
view() { $NEAR contract call-function as-read-only "$C" "$1" json-args "$2" network-config testnet now 2>&1 | sed -n '/^{/,/^}/p;/^\[/,/^\]/p'; }

echo "== both step up to the gate"
call "$A" enter_gate '{}' '0.1 NEAR'
call "$B" enter_gate '{}' '0.1 NEAR'
sleep 30
echo "== alice pays to skip after about 30 seconds"
call "$A" pass_gate '{}' '5 NEAR'
view get_holder "{\"account_id\":\"$A\"}"
echo "== bob tries to pass early without paying (must fail)"
call "$B" pass_gate '{}' '0 NEAR'
sleep 280
echo "== bob walks through for free"
call "$B" pass_gate '{}' '0 NEAR'
view get_holder "{\"account_id\":\"$B\"}"
echo "== bob sends 1000 SABIR to alice (taxed)"
call "$B" ft_transfer "{\"receiver_id\":\"$A\",\"amount\":\"1000000000000000000000\"}" '1 yoctoNEAR'
echo "== alice leaves with half her balance"
BAL=$($NEAR contract call-function as-read-only "$C" ft_balance_of json-args "{\"account_id\":\"$A\"}" network-config testnet now 2>&1 | grep -oE '"[0-9]+"' | head -1 | tr -d '"')
HALF=$(python -c "print($BAL // 2)")
call "$A" leave "{\"amount\":\"$HALF\"}" '0 NEAR'
view get_state '{}'
view get_log '{"limit":10}'
