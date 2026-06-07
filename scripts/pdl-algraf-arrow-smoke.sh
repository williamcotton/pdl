#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_LABEL="${1:-pdl_algraf_arrow_smoke}"
PDL_BIN="${PDL_BIN:-$ROOT/target/release/pdl}"
ALGRAF_BIN="${ALGRAF_BIN:-$ROOT/../algraf/target/release/algraf}"
if [[ ! -x "$ALGRAF_BIN" ]]; then
  ALGRAF_BIN="${ALGRAF_BIN_FALLBACK:-algraf}"
fi

PDL_PROGRAM="$ROOT/bench/workloads/large/pdl_to_algraf_arrow_handoff.pdl"
ALGRAF_CHART="$ROOT/bench/workloads/algraf/pdl_arrow_handoff.ag"
RUN_DIR="$ROOT/bench/runs/$RUN_LABEL"
ARROW_PATH="$RUN_DIR/pdl_to_algraf_arrow_handoff.arrow"
SVG_PATH="$RUN_DIR/pdl_to_algraf_arrow_handoff.svg"
PLAN_PATH="$RUN_DIR/pdl_plan.json"
REPORT_PATH="$RUN_DIR/pdl_algraf_smoke.csv"
INPUT_CSV="$ROOT/bench/data/generated/million-row.csv"
mkdir -p "$RUN_DIR"

now_ms() {
  python3 -c 'import time; print(int(time.time() * 1000))'
}

"$PDL_BIN" plan "$PDL_PROGRAM" --stdout-format arrow-stream --engine auto --json > "$PLAN_PATH"
PDL_ENGINE="$(python3 - "$PLAN_PATH" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    data = json.load(handle)
print(data.get("execution", {}).get("observability", {}).get("selected_engine", "unknown"))
PY
)"

start="$(now_ms)"
"$PDL_BIN" run "$PDL_PROGRAM" --engine auto --stdout-format arrow-stream > "$ARROW_PATH"
pdl_elapsed_ms="$(( $(now_ms) - start ))"
arrow_bytes="$(wc -c < "$ARROW_PATH" | tr -d ' ')"
input_rows="$(( $(wc -l < "$INPUT_CSV" | tr -d ' ') - 1 ))"

start="$(now_ms)"
"$ALGRAF_BIN" render "$ALGRAF_CHART" --data - --data-format arrow-stream --output "$SVG_PATH" < "$ARROW_PATH"
algraf_elapsed_ms="$(( $(now_ms) - start ))"
svg_bytes="$(wc -c < "$SVG_PATH" | tr -d ' ')"

{
  printf 'run_label,pdl_engine,input_rows,arrow_bytes,pdl_elapsed_ms,algraf_elapsed_ms,svg_bytes,status\n'
  printf '%s,%s,%s,%s,%s,%s,%s,ok\n' \
    "$RUN_LABEL" "$PDL_ENGINE" "$input_rows" "$arrow_bytes" "$pdl_elapsed_ms" "$algraf_elapsed_ms" "$svg_bytes"
} > "$REPORT_PATH"

printf 'wrote %s\n' "${REPORT_PATH#$ROOT/}"
