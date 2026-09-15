#!/usr/bin/env bash
#
# Regenerate a report, then normalise it.
#
# WHY THIS EXISTS
# ---------------
# `uzi` copies assets/report-template.html into the report essentially verbatim.
# That means two things land in the generated report that shouldn't:
#
#   1. the template's preview-layer `<script src=...template_preview_layer.js>`
#      tag — a report has no use for the preview layer, and the relative path
#      does not resolve from `reports/<dir>/`, so it is only a 404;
#   2. any CSS change outside the boot block, because `uzi` has no idea the
#      template evolved.
#
# So: lint the layer, generate, then run the three normalisation steps. All are
# idempotent, so re-running this is always safe.
#
# Usage:
#   tools/gen_report.sh <TICKER> [extra uzi args...]
#
# Example:
#   tools/gen_report.sh BTC-USD
#   tools/gen_report.sh 600519.SH --depth lite
set -euo pipefail

cd "$(dirname "$0")/.."

PY="${PY:-python3}"

if [ $# -lt 1 ]; then
  echo "usage: tools/gen_report.sh <TICKER> [extra uzi args...]" >&2
  exit 2
fi

ticker="$1"
shift

echo
echo "▶ linting preview layer"
"$PY" tools/lint_preview_layer.py

echo
echo "▶ uzi $ticker --stage2 --no-browser $*"
./uzi "$ticker" --stage2 --no-browser "$@"

echo
echo "▶ normalising"
"$PY" tools/sync_boot_intro.py
"$PY" tools/patch_report_css.py
"$PY" tools/rebuild_standalone.py reports/*/

echo
echo "✅ $ticker: generated and normalised"
