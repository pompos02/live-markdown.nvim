#!/usr/bin/env bash

set -euo pipefail

profile="${1:-release}"

if [[ "${profile}" == "release" ]]; then
    cargo build --release
    artifact_dir="target/release"
else
    cargo build
    artifact_dir="target/debug"
fi

case "$(uname -s)" in
    Linux)
        source_name="libmarkdown_render_native.so"
        target_name="markdown_render_native.so"
        ;;
    Darwin)
        source_name="libmarkdown_render_native.dylib"
        target_name="markdown_render_native.so"
        ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
        source_name="markdown_render_native.dll"
        target_name="markdown_render_native.dll"
        ;;
    *)
        echo "unsupported platform: $(uname -s)" >&2
        exit 1
        ;;
esac

mkdir -p lua
cp "${artifact_dir}/${source_name}" "lua/${target_name}"

echo "Built native module: lua/${target_name}"
