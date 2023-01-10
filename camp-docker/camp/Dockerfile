FROM alpine:3.16

WORKDIR /usr/bin/app

COPY camp .

RUN mkdir -p data
RUN mkdir -p static
RUN mkdir -p templates

COPY data/ ./data
COPY static/ ./static
COPY templates/ ./templates
COPY config.toml .
COPY wait-for .

ENV LOG_LEVEL=info

CMD ["./camp"]