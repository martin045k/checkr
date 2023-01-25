FROM rustlang/rust:nightly-slim as checko

ENV CARGO_TARGET_DIR=/.cargo-target

RUN apt-get update && apt-get install -y pkg-config libssl-dev

RUN cargo install --quiet cargo-binstall
RUN cargo binstall --no-confirm --quiet \
    just

COPY . /root/code

WORKDIR /root/code

RUN \
    --mount=type=cache,target=/.cargo-target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    cargo build --release -p checko && \
    cp /.cargo-target/release/checko /root/checko

FROM mcr.microsoft.com/dotnet/sdk:7.0-bullseye-slim

COPY --from=checko /root/checko /usr/bin
