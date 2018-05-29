#!/usr/bin/env bash
set -eux

DESTDIR=/src/releases

mkdir -p $DESTDIR/i386-linux
rm -f $DESTDIR/i386-linux/*

mkdir -p $DESTDIR/arm-linux
rm -f $DESTDIR/arm-linux/*

function build {
	echo Building for $1 to $3...
	cargo build --release --target $1 \
		&& $2 /build/$1/release/spotty \
		&& cp /build/$1/release/spotty $DESTDIR/$3
}

build x86_64-unknown-linux-musl strip i386-linux/spotty-x86_64
build i686-unknown-linux-musl strip i386-linux/spotty
build aarch64-unknown-linux-gnu aarch64-linux-gnu-strip arm-linux/spotty-aarch64

# binary built in docker would not run on 1st gen. Pi running pCP. Whysoever.
# build arm-unknown-linux-gnueabihf arm-linux-gnueabihf-strip arm-linux/spotty-hf

# armel binary wouldn't run on eg. Synology due to wrong glibc version?
# build arm-unknown-linux-gnueabi arm-linux-gnueabi-strip arm-linux/spotty
