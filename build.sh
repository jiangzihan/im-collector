#!/usr/bin/env bash
set -e
R="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
( cd "$R/src" && cargo build --release )
for b in imc-wechat imc-dingtalk; do cp "$R/src/target/release/$b" "$R/bin/.$b.new" && mv -f "$R/bin/.$b.new" "$R/bin/$b"; done
echo "done -> $R/bin"
