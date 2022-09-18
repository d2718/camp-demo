FROM pandoc/latex:2.19

WORKDIR /usr/bin/camp_render

COPY target/x86_64-unknown-linux-musl/release/pandocker .
COPY deploy/pandocker.toml .

ENTRYPOINT ["/usr/bin/env"]
CMD ["./pandocker"]