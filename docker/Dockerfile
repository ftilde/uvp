FROM rust:1-alpine3.22 as builder
WORKDIR /usr/src/uvp
COPY . .
RUN apk add --no-cache musl-dev openssl-dev pkgconf sqlite-dev
ENV RUSTFLAGS='-C target-feature=-crt-static'
RUN cargo install --path uvp-server

FROM alpine:3.22
COPY --from=builder /usr/local/cargo/bin/uvp-server /usr/local/bin/uvp-server
RUN apk add --no-cache libgcc sqlite-libs
EXPOSE 3000
RUN mkdir -p /var/data
CMD ["uvp-server", "/var/data/uvp.db"]
