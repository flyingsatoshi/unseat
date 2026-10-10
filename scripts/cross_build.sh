#!/usr/bin/env bash
# Cross-compiles the release exe for x86_64-pc-windows-gnu inside a Rust Linux container.
# scripts/install.ps1 -Docker runs this with the repository mounted at /src; the result is
# target/x86_64-pc-windows-gnu/release/unseat.exe (a development path, so it never registers
# itself to start with Windows). Useful when Windows Smart App Control blocks freshly built
# build scripts and proc macros (os error 4551) on the host.
set -euo pipefail
apt-get update -qq >/dev/null
apt-get install -y -qq gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 >/dev/null 2>&1
rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1

out=/src/target/x86_64-pc-windows-gnu/release
mkdir -p /build "$out"
cd /src
tar --exclude=./target --exclude=./dist --exclude=./.git -cf - . | tar -xf - -C /build
cd /build
# Linux windres drops the quotes around -D string macros, so spell the version out here.
ver=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
sed -i "s/, UNSEAT_VERSION\$/, \"$ver\"/" unseat.rc

cargo build --locked --release --features gui --target x86_64-pc-windows-gnu --target-dir /tmp/target
x86_64-w64-mingw32-strip -o "$out/unseat.exe" /tmp/target/x86_64-pc-windows-gnu/release/unseat.exe
echo "Built target/x86_64-pc-windows-gnu/release/unseat.exe ($ver)"
