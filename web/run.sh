#!/usr/bin/env bash
export IMC_HOME="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$IMC_HOME/web/serve.py"
