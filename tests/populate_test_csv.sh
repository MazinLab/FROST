#!/usr/bin/env bash
# populate_test_csv.sh — Simulates the temperature recording loop for GL7 testing.
#
# Writes the header and separator from the source CSV immediately, then appends
# one data row every 33 seconds, matching the real `frost record-temps loop` rate.
# Rows are sourced from the March 19 temperature log with lines 50–153 skipped
# so Phase 3 conditions are reached faster during testing.
#
#   Source segments replayed:
#     lines 3–49   (Phase 1 / early Phase 2 data)
#     lines 154–381 (late Phase 2 / Phase 3 data — skips slow mid-Phase 2 plateau)
#
# Usage:
#   ./tests/populate_test_csv.sh
#
# The output file is written to FROST/temps/test_csv.csv. Run this in one
# terminal, then run `frost gl7 cooldown --csv temps/test_csv.csv` in another.

set -euo pipefail

SOURCE="temps/2026-03-19_temperature_log.csv"
OUTPUT="temps/test_csv.csv"
INTERVAL=33   # seconds between rows — matches real recording rate
LAST_LINE=381 # last line of the source file to replay

# Resolve paths relative to the FROST repo root regardless of where the script
# is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
SOURCE="$REPO_ROOT/$SOURCE"
OUTPUT="$REPO_ROOT/$OUTPUT"

if [[ ! -f "$SOURCE" ]]; then
    echo "ERROR: source CSV not found: $SOURCE" >&2
    exit 1
fi

# Write header + separator immediately so the controller can open the file.
echo "Writing header to $OUTPUT"
head -n 2 "$SOURCE" > "$OUTPUT"

# Stream data rows one at a time, skipping lines 50–153.
# lines 3–49:   47 rows  (Phase 1 / early Phase 2 data)
# lines 154–LAST_LINE: (LAST_LINE - 153) rows  (late Phase 2 / Phase 3)
TOTAL=$(( 47 + LAST_LINE - 153 ))
COUNT=0

while IFS= read -r line; do
    COUNT=$(( COUNT + 1 ))
    echo "$line" >> "$OUTPUT"
    echo "[$(date '+%H:%M:%S')]  appended row $COUNT / $TOTAL"
    if [[ $COUNT -lt $TOTAL ]]; then
        sleep "$INTERVAL"
    fi
done < <(sed -n "3,49p; 154,${LAST_LINE}p" "$SOURCE")

echo "Done — all $TOTAL rows written to $OUTPUT"
