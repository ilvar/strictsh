#!/bin/sh
set -u

if result=$(sh main.sh); then
    if [ "${result}" = 'Hello from __NAME__.' ]; then
        printf 'smoke test passed\n'
    else
        printf 'unexpected output: %s\n' "${result}" >&2 || exit 1
        exit 1
    fi
else
    printf 'main.sh failed\n' >&2 || exit 1
    exit 1
fi
