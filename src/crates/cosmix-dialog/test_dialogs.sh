#!/bin/bash
# Test all cosmix-dialog CLI types sequentially.
# Each dialog blocks until dismissed, then prints its result.

set -e

echo "=== 1. Info message ==="
cosmix-dialog info --text "This is an info message"
echo "rc=$?"

echo "=== 2. Warning message ==="
cosmix-dialog warning --text "This is a warning"
echo "rc=$?"

echo "=== 3. Error message ==="
cosmix-dialog error --text "This is an error"
echo "rc=$?"

echo "=== 4. Confirm (yes/no) ==="
cosmix-dialog confirm --text "Do you want to continue?"
echo "rc=$?"

echo "=== 5. Entry (text input) ==="
result=$(cosmix-dialog input --text "What is your name?" --entry-text "World")
echo "result='$result' rc=$?"

echo "=== 6. Password ==="
result=$(cosmix-dialog password --text "Enter your secret:")
echo "result='$result' rc=$?"

echo "=== 7. ComboBox ==="
result=$(cosmix-dialog combo --text "Pick a colour:" --items red green blue yellow)
echo "result='$result' rc=$?"

echo "=== 8. CheckList ==="
result=$(cosmix-dialog checklist --text "Enable features:" --items "logs:Logs:on" "metrics:Metrics:off" "traces:Traces:off")
echo "result='$result' rc=$?"

echo "=== 9. RadioList ==="
result=$(cosmix-dialog radiolist --text "Select mode:" --items "fast:Fast:on" "safe:Safe:off" "debug:Debug:off")
echo "result='$result' rc=$?"

echo "=== 10. Text input (multi-line) ==="
result=$(cosmix-dialog text-input --text "Edit your notes:" --default "Line one\nLine two")
echo "result='$result' rc=$?"

echo "=== 11. Progress (pulsating, 3s) ==="
(for i in $(seq 1 3); do sleep 1; echo $((i * 33)); done; echo 100) | cosmix-dialog progress --text "Working..." --auto-close
echo "rc=$?"

echo "=== All tests complete ==="
