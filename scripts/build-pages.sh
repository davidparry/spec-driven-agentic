#!/usr/bin/env bash
# Assemble the GitHub Pages site into _site/.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/build-pages.py
