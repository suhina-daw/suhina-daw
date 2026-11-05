#!/usr/bin/env bash
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Checks that a release's folder holds every platform's package and its checksum before the
# release workflow publishes it (.github/workflows/release.yml).
#
#   scripts/release_packages_check.sh DIST NAME
#
# DIST is the folder the artifacts were downloaded into, NAME the version part of the package
# names (suhina-NAME-linux-x86_64.tar.gz and so on). Exits 1 and names what is missing when a
# package is not there, for example after one build was re-run once the others' artifacts expired.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 DIST NAME" >&2
  exit 2
fi
dist=$1
name=$2

# One per platform the build job's matrix makes.
expected=(
  "suhina-$name-linux-x86_64.tar.gz"
  "suhina-$name-windows-x86_64.zip"
  "suhina-$name-macos-universal.tar.gz"
)

missing=0
for package in "${expected[@]}"; do
  for file in "$package" "$package.sha256"; do
    if [ ! -s "$dist/$file" ]; then
      echo "::error::$file is missing: re-run the whole workflow, not one build" >&2
      missing=1
    fi
  done
done
exit "$missing"
