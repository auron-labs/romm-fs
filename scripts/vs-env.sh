#!/usr/bin/env bash
# MSVC BuildTools + Windows SDK environment for building this repo on the
# Windows dev boxes. `vcvars*.bat`/`VsDevShell` hang under Git Bash, so this
# sets INCLUDE/LIB/PATH directly. It discovers the installed MSVC and SDK
# versions, so it survives toolchain updates. Source it:
#
#   source scripts/vs-env.sh
#
# MSVC's link.exe is prepended to PATH so it wins over Git's (which cargo
# would otherwise pick up and fail with /link vs /LTCG confusion).

set -e

MSVC_ROOT="$(
    ls -d /c/BuildTools/VC/Tools/MSVC/*/ 2>/dev/null | sort -V | tail -1 | sed 's:/$::'
)"
[ -n "$MSVC_ROOT" ] || { echo "vs-env.sh: no MSVC under C:\\BuildTools" >&2; return 1; }

SDK_ROOT='/c/Program Files (x86)/Windows Kits/10'
SDK_VER="$(
    ls -d "$SDK_ROOT/Include/"*/ 2>/dev/null | xargs -n1 basename 2>/dev/null | sort -V | tail -1
)"
[ -n "$SDK_VER" ] || { echo "vs-env.sh: no Windows SDK under $SDK_ROOT" >&2; return 1; }

MSVC_WIN="$(cygpath -w "$MSVC_ROOT")"
SDK_WIN="$(cygpath -w "$SDK_ROOT")"

export INCLUDE="$MSVC_WIN\\include;$SDK_WIN\\Include\\$SDK_VER\\ucrt;$SDK_WIN\\Include\\$SDK_VER\\um;$SDK_WIN\\Include\\$SDK_VER\\shared;$SDK_WIN\\Include\\$SDK_VER\\winrt"
export LIB="$MSVC_WIN\\lib\\x64;$SDK_WIN\\Lib\\$SDK_VER\\ucrt\\x64;$SDK_WIN\\Lib\\$SDK_VER\\um\\x64"
export PATH="$MSVC_ROOT/bin/Hostx64/x64:$SDK_ROOT/bin/$SDK_VER/x64:$PATH"
