#!/usr/bin/env bash
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Installs cargo-nextest, the test runner CI runs the workspace's tests with
# (.github/workflows/ci.yml, .config/nextest.toml), from its release on GitHub, next to `cargo`.
#
#   scripts/install_nextest.sh
#
# Pinned, so a new release cannot change the result of a pull request by itself: to update, change
# the version and the checksums of its three archives together, taking a release that is at least
# a week old (CONTRIBUTING.md › Dependencies). cargo-nextest is Apache-2.0 OR MIT.
set -euo pipefail

VERSION="0.9.151"

case "$(uname -s)" in
  Linux)
    target="x86_64-unknown-linux-gnu"
    sha256="1d4ce302b820c41f4b89c5fb9bce45f0f0c94882c1cf08fc360caf08be6e7df4"
    ;;
  Darwin)
    target="universal-apple-darwin"
    sha256="d1d7d025d4bfc9fb154cd76aea3949bd78534829a0589abefa6e5dd4edb135dd"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    target="x86_64-pc-windows-msvc"
    sha256="e51bdb4b5883df48f335381b287378edb244318fac65ae0adaf6b9e69298246b"
    ;;
  *)
    echo "No cargo-nextest archive is pinned for $(uname -s)." >&2
    exit 1
    ;;
esac

name="cargo-nextest-${VERSION}-${target}.tar.gz"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -sSfL -o "$work/$name" \
  "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${VERSION}/${name}"
if command -v sha256sum >/dev/null; then
  echo "${sha256}  $work/$name" | sha256sum --check
else
  echo "${sha256}  $work/$name" | shasum -a 256 --check
fi
tar -xzf "$work/$name" -C "$work"

bin="$(dirname "$(command -v cargo)")"
if [ -f "$work/cargo-nextest.exe" ]; then
  cp "$work/cargo-nextest.exe" "$bin/"
else
  install -m 755 "$work/cargo-nextest" "$bin/cargo-nextest"
fi
cargo nextest --version
