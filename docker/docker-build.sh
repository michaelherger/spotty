#!/usr/bin/env bash
set -eux

cp -r /src/src /src/*.rs /src/Cargo.* .

DESTDIR=/src/releases

mkdir -p $DESTDIR/i386-linux
mkdir -p $DESTDIR/arm-linux

cargo build --release --target x86_64-unknown-linux-musl \
	&& strip /build/x86_64-unknown-linux-musl/release/spotty \
	&& cp /build/x86_64-unknown-linux-musl/release/spotty $DESTDIR/i386-linux/spotty-x86_64

cargo build --release --target i686-unknown-linux-musl \
	&& strip /build/i686-unknown-linux-musl/release/spotty \
	&& cp /build/i686-unknown-linux-musl/release/spotty $DESTDIR/i386-linux/spotty

cargo build --release --target aarch64-unknown-linux-gnu \
	&& aarch64-linux-gnu-strip /build/aarch64-unknown-linux-gnu/release/spotty \
	&& cp /build/aarch64-unknown-linux-gnu/release/spotty $DESTDIR/arm-linux/spotty-aarch64

cargo build --release --target arm-unknown-linux-gnueabihf \
	&& arm-linux-gnueabihf-strip /build/arm-unknown-linux-gnueabihf/release/spotty \
	&& cp /build/arm-unknown-linux-gnueabihf/release/spotty $DESTDIR/arm-linux/spotty-hf
