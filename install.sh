#!/bin/sh
set -u

REPOSITORY='https://github.com/ilvar/strictsh'

error() {
    printf 'strictsh installer: %s\n' "${1}" >&2 || return 1
}

target_for_host() {
    case "${1}:${2}" in
        Linux:x86_64|Linux:amd64) printf 'x86_64-unknown-linux-musl' ;;
        Linux:aarch64|Linux:arm64) printf 'aarch64-unknown-linux-musl' ;;
        Darwin:x86_64|Darwin:amd64) printf 'x86_64-apple-darwin' ;;
        Darwin:aarch64|Darwin:arm64) printf 'aarch64-apple-darwin' ;;
        *) printf '' ;;
    esac
}

verify_checksum() {
    if sha256sum --version >/dev/null 2>&1; then
        sha256sum --check --strict "${1}" || return 1
    else
        shasum -a 256 --check "${1}" || return 1
    fi
}

install_release() {
    workdir="${1}"
    if [ -z "${STRICTSH_INSTALL_DIR:-}" ] && [ -z "${HOME:-}" ]; then
        error 'set HOME or STRICTSH_INSTALL_DIR to choose a destination' || return 1
        return 1
    fi
    install_dir="${STRICTSH_INSTALL_DIR:-${HOME:-}/.local/bin}"
    os=$(uname -s) || return 1
    arch=$(uname -m) || return 1
    target=$(target_for_host "${os}" "${arch}") || return 1
    version="${STRICTSH_VERSION:-latest}"
    if [ "${version}" = latest ]; then
        latest_url=$(curl --fail --silent --show-error --location --retry 3 --connect-timeout 10 --max-time 60 \
            --proto '=https' --proto-redir '=https' --output /dev/null --write-out '%{url_effective}' \
            "${REPOSITORY}/releases/latest") || return 1
        tag="${latest_url##*/}"
    else
        tag="v${version#v}"
    fi
    case "${tag}" in
        v[0-9]*.[0-9]*.[0-9]*) ;;
        *) error 'STRICTSH_VERSION must be latest or a version such as 0.1.0 or v0.1.0' || return 1; return 1 ;;
    esac
    version_numbers="${tag#v}"
    version_tail="${version_numbers#*.}"
    version_patch="${version_tail#*.}"
    case "${version_numbers}:${version_patch}" in
        *[!0-9.:]*|*:*.*) error 'STRICTSH_VERSION must be latest or a version with exactly three numeric components' || return 1; return 1 ;;
        *) : ;;
    esac

    if [ -n "${target}" ] && [ "${STRICTSH_BUILD_FROM_SOURCE:-0}" = 0 ]; then
        asset="strictsh-${target}"
        base_url="${REPOSITORY}/releases/download/${tag}"
        curl --fail --silent --show-error --location --retry 3 --connect-timeout 10 --max-time 120 \
            --proto '=https' --proto-redir '=https' --output "${workdir}/${asset}" \
            "${base_url}/${asset}" || return 1
        curl --fail --silent --show-error --location --retry 3 --connect-timeout 10 --max-time 60 \
            --proto '=https' --proto-redir '=https' --output "${workdir}/${asset}.sha256" \
            "${base_url}/${asset}.sha256" || return 1
        # The checksum's filename is relative to the isolated download directory.
        if (cd "${workdir}" && verify_checksum "${asset}.sha256" || exit 1); then
            binary="${workdir}/${asset}"
        else
            error 'checksum verification failed; refusing to install' || return 1
            return 1
        fi
    else
        printf 'Building strictsh %s for %s/%s from source.\n' "${tag}" "${os}" "${arch}"
        if rustup --version >/dev/null 2>&1 && cargo --version >/dev/null 2>&1; then
            rustup toolchain install 1.97.1 --profile minimal || return 1
            cargo +1.97.1 install --git "${REPOSITORY}" --tag "${tag}" --locked \
                --root "${workdir}/build" || return 1
            binary="${workdir}/build/bin/strictsh"
        else
            error 'source builds require rustup, cargo and a native C compiler; install Rust from https://rustup.rs first' || return 1
            return 1
        fi
    fi

    mkdir -p -- "${install_dir}" || return 1
    install -m 0755 -- "${binary}" "${install_dir}/strictsh" || return 1
    printf 'Installed strictsh %s to %s/strictsh\n' "${tag}" "${install_dir}"
    printf 'Ensure %s is on PATH.\n' "${install_dir}"
    if shellcheck --version >/dev/null 2>&1; then
        :
    else
        printf 'ShellCheck is required for linting; install it using your system package manager.\n' >&2 || return 1
    fi
}

main() {
    if [ "${#}" -ne 0 ]; then
        error 'usage: sh install.sh (configure STRICTSH_VERSION, STRICTSH_INSTALL_DIR or STRICTSH_BUILD_FROM_SOURCE=1 via environment)' || return 1
        return 1
    fi
    workdir=$(mktemp -d "${TMPDIR:-/tmp}/strictsh-install.XXXXXXXX") || return 1
    if install_release "${workdir}"; then
        result=0
    else
        result=1
    fi
    rm -rf -- "${workdir}" || return 1
    return "${result}"
}

main "${@}" || exit 1
