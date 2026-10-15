#!/usr/bin/env bash
# Cross-compiles the release exe for x86_64-pc-windows-gnu inside a Rust Linux container.
# Optional: only scripts/install.ps1 -Docker runs this, with the repository mounted at /src
# and CPU/memory limits; the result is target/x86_64-pc-windows-gnu/release/unseat.exe, a
# staging copy that install.ps1 moves to dist\ and deletes. The exe needs only Windows' own
# DLLs; Docker is never needed to install or run Unseat.
set -euo pipefail

# Run a setup step quietly, but say which one failed and show its output instead of exiting
# silently. Downloads are retried: a dropped connection is the usual cause.
step() {
  local what=$1
  shift
  for attempt in 1 2 3; do
    if "$@" >/tmp/step.log 2>&1; then
      return 0
    fi
    echo "cross_build: $what failed (attempt $attempt of 3)" >&2
    [ "$attempt" -lt 3 ] && sleep $((attempt * 5))
  done
  cat /tmp/step.log >&2
  echo "cross_build: giving up: $what" >&2
  exit 1
}

step "apt-get update" apt-get update -qq
step "installing MinGW" apt-get install -y -qq gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64
step "adding the Windows target" rustup target add x86_64-pc-windows-gnu

out=/src/target/x86_64-pc-windows-gnu/release
mkdir -p /build "$out"
cd /src
tar --exclude=./target --exclude=./dist --exclude=./.git --exclude=./spikes -cf - . | tar -xf - -C /build
cd /build
# A Windows checkout (core.autocrlf) can have CRLF line ends. The files the Linux tools read
# or edit are normalized in this copy only; the working tree is left alone.
sed -i 's/\r$//' Cargo.toml unseat.rc unseat.manifest
# Linux windres drops the quotes around -D string macros, so spell the version out here.
ver=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
sed -i "s/, UNSEAT_VERSION\$/, \"$ver\"/" unseat.rc
if ! grep -q "\"$ver\"" unseat.rc; then
  echo "cross_build: could not put version $ver into unseat.rc" >&2
  exit 1
fi

cargo build --locked --release --features gui --target x86_64-pc-windows-gnu --target-dir /tmp/target
x86_64-w64-mingw32-strip -o "$out/unseat.exe" /tmp/target/x86_64-pc-windows-gnu/release/unseat.exe
echo "Built target/x86_64-pc-windows-gnu/release/unseat.exe ($ver)"
