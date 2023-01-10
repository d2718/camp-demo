from postgres:14-alpine

RUN mkdir -p /docker-entrypoint-initdb.d
COPY create-db.sql /docker-entrypoint-initdb.d/

ENV POSTGRES_HOST_AUTH_METHOD=trust

CMD ["postgres"]

# docker build -t camp-postgres .
# docker run -it --rm -p 5000:5432 camp-postgres