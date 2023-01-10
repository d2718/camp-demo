FROM pandoc/latex:2.19

WORKDIR /usr/bin/camp_render

COPY pandocker .
COPY pandocker.toml .

ENTRYPOINT ["/usr/bin/env"]
CMD ["./pandocker"]