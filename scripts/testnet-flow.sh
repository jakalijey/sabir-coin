#!/usr/bin/env bash
# End-to-end check on testnet: one walker skips, one waits, then collecting, a sale, a broken soul and an exit.
set -u
NEAR=${NEAR:-near}
C=${CONTRACT:?set CONTRACT to the deployed account}
A=${ALICE:?set ALICE to a testnet account}
B=${BOB:?set BOB to a testnet account}
call() { $NEAR contract call-function as-transaction "$C" "$2" json-args "$3" prepaid-gas '100.0 Tgas' attached-deposit "$4" sign-as "$1" network-config testnet sign-with-legacy-keychain send 2>&1 | grep -E "succeeded|failed|Error|panicked|Smart contract" | head -3; }
view() { $NEAR contract call-function as-read-only "$C" "$1" json-args "$2" network-config testnet now 2>&1 | sed -n '/^{/,/^}/p;/^\[/,/^\]/p'; }

echo "== both step onto the bridge"
call "$A" enter_gate '{}' '1 NEAR'
call "$B" enter_gate '{}' '1 NEAR'
sleep 30
echo "== alice pays to skip after about 30 seconds"
call "$A" pass_gate '{}' '1 NEAR'
view get_soul '{"token_id":"1"}'
echo "== bob tries to pass early without paying (must fail)"
call "$B" pass_gate '{}' '0 NEAR'
sleep 280
echo "== bob walks through for free"
call "$B" pass_gate '{}' '0 NEAR'
view get_state '{}'
sleep 60
echo "== alice collects what her soul has earned"
call "$A" collect '{"token_id":"1"}' '0 NEAR'
view get_holder "{\"account_id\":\"$A\"}"
echo "== alice puts her soul up for sale and bob buys it"
call "$A" list_soul '{"token_id":"1","price":"1500000000000000000000000"}' '1 yoctoNEAR'
call "$B" buy_soul '{"token_id":"1","price":"1500000000000000000000000"}' '1.505 NEAR'
view get_soul '{"token_id":"1"}'
echo "== bob breaks a soul because he got bored"
call "$B" break_soul '{"token_id":"2","reason":3}' '1 yoctoNEAR'
view get_exit_book '{}'
echo "== bob leaves with half of his SABIR"
BAL=$($NEAR contract call-function as-read-only "$C" ft_balance_of json-args "{\"account_id\":\"$B\"}" network-config testnet now 2>&1 | grep -oE '"[0-9]+"' | head -1 | tr -d '"')
HALF=$(python -c "print($BAL // 2)")
call "$B" leave "{\"amount\":\"$HALF\"}" '1 yoctoNEAR'
view get_state '{}'
view get_log '{"limit":10}'
