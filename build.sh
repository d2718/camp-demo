#!/bin/sh

set -xeu pipefail

TARGET=x86_64-unknown-linux-musl
DIR=target/$TARGET/release

cargo build --target $TARGET --release

for PROG in camp pandocker sendgrid_mock; do
    BIN=$DIR/$PROG
    DEST=camp-docker/$PROG/$PROG
    strip $BIN
    cp $BIN $DEST
done

POPBIN=target/$TARGET/release/demo_data
strip $POPBIN
cp $POPBIN ./