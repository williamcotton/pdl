#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/pdl"
OUT="$ROOT/bench-output/large-demos"
REPORT="$OUT/report.tsv"
REPORT_HEADER=$'demo\tstatus\tformat\toutput\trows\tbytes\telapsed_ms\trun_timestamp_utc\tgit_ref'
RUN_TIMESTAMP_UTC="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
RUN_GIT_REF="$(git -C "$ROOT" describe --tags --always --dirty 2>/dev/null || printf 'unknown')"

mkdir -p "$OUT"

if [[ ! -f "$ROOT/benchdata/generated/million-row.csv" ]]; then
    "$ROOT/scripts/generate-million-row-csv.sh"
fi

cd "$ROOT"
cargo build -p pdl-cli

if [[ ! -s "$REPORT" ]]; then
    printf '%s\n' "$REPORT_HEADER" > "$REPORT"
elif ! head -n 1 "$REPORT" | grep -qx "$REPORT_HEADER"; then
    printf 'warning: %s already exists with a different header; appending rows with current schema\n' "$REPORT" >&2
fi

now_ms() {
    if command -v python3 >/dev/null 2>&1; then
        python3 -c 'import time; print(time.time_ns() // 1000000)'
    elif command -v perl >/dev/null 2>&1; then
        perl -MTime::HiRes=time -e 'printf "%.0f\n", time() * 1000'
    else
        printf '%s000\n' "$(date +%s)"
    fi
}

byte_count() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        printf '0'
        return
    fi
    wc -c < "$path" | tr -d ' '
}

csv_row_count() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        printf '0'
        return
    fi
    awk 'END { if (NR == 0) print 0; else print NR - 1 }' "$path"
}

extension_for_format() {
    case "$1" in
        csv) printf 'csv' ;;
        jsonl) printf 'jsonl' ;;
        arrow-stream) printf 'arrow' ;;
        arrow-file) printf 'arrow' ;;
        parquet) printf 'parquet' ;;
        *) printf 'out' ;;
    esac
}

run_demo() {
    local program="$1"
    local format="$2"
    local name
    name="$(basename "$program" .pdl)"
    local ext
    ext="$(extension_for_format "$format")"
    local label="${name}-${format}"
    local out="$OUT/$label.$ext"
    local log="$OUT/$label.log"
    local start_ms end_ms elapsed_ms rows bytes
    start_ms="$(now_ms)"
    if "$BIN" run "$program" --stdout-format "$format" >"$out" 2>"$log"; then
        end_ms="$(now_ms)"
        elapsed_ms=$((end_ms - start_ms))
        bytes="$(byte_count "$out")"
        if [[ "$format" == "csv" ]]; then
            rows="$(csv_row_count "$out")"
        else
            rows="-"
        fi
        printf '%s\tok\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$format" "$out" "$rows" "$bytes" "$elapsed_ms" "$RUN_TIMESTAMP_UTC" "$RUN_GIT_REF" | tee -a "$REPORT"
    else
        end_ms="$(now_ms)"
        elapsed_ms=$((end_ms - start_ms))
        printf '%s\tfailed\t%s\t%s\t0\t0\t%s\t%s\t%s\n' "$name" "$format" "$log" "$elapsed_ms" "$RUN_TIMESTAMP_UTC" "$RUN_GIT_REF" | tee -a "$REPORT"
        return 1
    fi
}

failures=0

run_demo bench/examples/large/million_row_segment_summary.pdl csv || failures=$((failures + 1))
run_demo bench/examples/large/million_row_segment_summary.pdl arrow-stream || failures=$((failures + 1))
run_demo bench/examples/large/million_row_top_scores.pdl csv || failures=$((failures + 1))
run_demo bench/examples/large/million_row_projection_smoke.pdl csv || failures=$((failures + 1))
run_demo bench/examples/large/million_row_distinct_segments.pdl csv || failures=$((failures + 1))

printf '\nLarge demo report: %s\n' "$REPORT"

if [[ "$failures" -gt 0 ]]; then
    exit 1
fi
