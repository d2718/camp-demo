FROM alpine:3.16

WORKDIR /usr/bin/app

COPY target/x86_64-unknown-linux-musl/release/camp .

RUN mkdir -p data
RUN mkdir -p static
RUN mkdir -p templates

COPY data/ ./data
COPY static/ ./static
COPY templates/ ./templates

COPY deploy/config.toml .

ENV LOG_LEVEL=info

CMD ["./camp"]