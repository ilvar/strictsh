#!/bin/sh
set -u

main() {
    printf 'Hello from __NAME__.\n'
}

main "${@}" || exit 1
