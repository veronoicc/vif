FROM --platform=$BUILDPLATFORM rust:alpine AS chef
WORKDIR /app
ENV PKGCONFIG_SYSROOTDIR=/
RUN apk add --no-cache musl-dev openssl-dev zig
RUN cargo install --locked cargo-zigbuild cargo-chef
RUN rustup default nightly && \
    rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
 
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json
 
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json --release --zigbuild \
  --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl
 

COPY . .
RUN cargo zigbuild -r --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl && \
  mkdir /app/linux && \
  cp target/aarch64-unknown-linux-musl/release/vif /app/linux/arm64 && \
  cp target/x86_64-unknown-linux-musl/release/vif /app/linux/amd64

FROM alpine:latest AS runtime
WORKDIR /app
ARG TARGETPLATFORM
ENV PKGCONFIG_SYSROOTDIR=/
COPY --from=builder /app/${TARGETPLATFORM} /app/vif
ENTRYPOINT ["/app/vif"]