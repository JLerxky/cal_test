FROM rust:slim-bullseye AS buildstage
WORKDIR /build
ENV PROTOC_NO_VENDOR 1
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config libssl-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*
COPY . /build/
RUN cargo build --release

FROM debian:bullseye-slim
# get the latest CA certs
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates vim \
    && update-ca-certificates \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -m test
USER test
COPY --from=buildstage /build/target/release/cal_test /usr/bin/
CMD ["cal_test"]
