#!/usr/bin/bash

set -xeuo pipefail

TARGET=x86_64-unknown-linux-musl
BINARY=target/$TARGET/release/pandocker
ARTIFACT=us-east1-docker.pkg.dev/camp-357714/camp-repo/pandocker

cargo build --target $TARGET --release
strip $BINARY

docker build -f pandoc.Dockerfile -t pandocker -t $ARTIFACT .
docker push $ARTIFACT

