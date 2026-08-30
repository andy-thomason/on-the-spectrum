#!/bin/sh
# Fetch the SingleStepTests/z80 per-opcode test vectors used by tests/z80_json.rs.
#
# 1604 JSON files, 1000 tests each: initial and final CPU state, memory, and the bus
# activity of every T-state. About 280 MB to download and 1.3 GB on disk. They are MIT
# licensed but far too large to vendor, so they live here untracked.
#
# Remove them with:  rm -rf tests/vectors/z80
set -eu

dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if [ -d "$dir/z80/v1" ]; then
    echo "have    $(ls "$dir/z80/v1" | wc -l) vector files"
    exit 0
fi

echo "cloning SingleStepTests/z80 (about 280 MB)"
git clone --depth 1 https://github.com/SingleStepTests/z80.git "$dir/z80"
rm -rf "$dir/z80/.git" "$dir/z80/generation" "$dir/z80/.github"
echo "have    $(ls "$dir/z80/v1" | wc -l) vector files"
