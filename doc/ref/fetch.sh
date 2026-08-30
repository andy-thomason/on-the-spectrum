#!/bin/sh
# Fetch the third-party reference PDFs that are not redistributed in this repository.
# See README.md in this directory for why.
set -eu

dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

fetch() {
    url=$1; out=$2
    if [ -s "$dir/$out" ]; then
        echo "have    $out"
        return
    fi
    echo "fetching $out"
    # -k: z80.info presents a self-signed certificate on HTTPS.
    curl -kfsSL --max-time 120 -o "$dir/$out" "$url" \
        || { echo "FAILED  $out  ($url)" >&2; rm -f "$dir/$out"; return 1; }
}

rc=0
fetch "http://www.z80.info/zip/z80cpu_um.pdf" "z80cpu_um.pdf" || rc=1
fetch "https://raw.githubusercontent.com/floooh/emu-info/master/z80/z80-documented.pdf" \
      "z80-documented.pdf" || rc=1

exit $rc
