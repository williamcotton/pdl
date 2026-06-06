#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROWS="${PDL_MILLION_ROW_COUNT:-1000000}"
OUT="$ROOT/benchdata/generated/million-row.csv"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --rows)
            ROWS="$2"
            shift 2
            ;;
        --out)
            OUT="$2"
            shift 2
            ;;
        -h|--help)
            cat <<'USAGE'
Usage: scripts/generate-million-row-csv.sh [--rows N] [--out PATH]

Generates a deterministic CSV benchmark fixture. Defaults to 1,000,000 rows at
benchdata/generated/million-row.csv, which is ignored by git.
USAGE
            exit 0
            ;;
        *)
            echo "unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

case "$ROWS" in
    ''|*[!0-9]*)
        echo "--rows must be a positive integer" >&2
        exit 2
        ;;
esac
if [[ "$ROWS" -eq 0 ]]; then
    echo "--rows must be a positive integer" >&2
    exit 2
fi

mkdir -p "$(dirname "$OUT")"
TMP="$OUT.tmp"

awk -v rows="$ROWS" '
BEGIN {
    print "row,segment,x,score,latency_ms"
    for (i = 0; i < rows; i++) {
        segment_index = i % 4
        segment = substr("ABCD", segment_index + 1, 1)
        x = (i % 10000) / 100.0
        cycle = (i * 37) % 1000
        drift = int(i / 100000)
        score = 20 + (segment_index * 5) + (x * 0.3) + (cycle / 25.0) + drift
        latency = 40 + (segment_index * 12) + (((i * 17) % 900) / 3.0)
        printf "%d,%s,%.2f,%.3f,%.3f\n", i, segment, x, score, latency
    }
}
' > "$TMP"

mv "$TMP" "$OUT"
printf 'Generated %s rows at %s\n' "$ROWS" "$OUT"
