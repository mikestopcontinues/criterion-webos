FROM public.ecr.aws/docker/library/rust:1.94.0-slim-bookworm@sha256:a86cada82e36ebd7a9bffed7548792c55a952fdb20718eea9278a936bcb76e62 AS development

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates curl bzip2 build-essential cmake pkg-config python3 file binutils \
       libsdl2-dev libegl1-mesa-dev libgles2-mesa-dev xvfb xauth mesa-utils \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt clippy \
    && rustup toolchain install nightly-2026-07-02 --profile minimal --component rust-src --component rustfmt --component clippy

ENV CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/target
WORKDIR /workspace

FROM development AS native
ARG TARGETARCH
RUN test "$TARGETARCH" = arm64
ADD https://github.com/webosbrew/native-toolchain/releases/download/webos-d7ed7ee.6/arm-webos-linux-gnueabi_sdk-buildroot_linux-aarch64.tar.bz2 /tmp/webos-ndk.tar.bz2
RUN printf '%s  %s\n' '45a2d12ff557457d92cde4fddaa77a6f1090fca03adc43bb74397e5e0c379501' '/tmp/webos-ndk.tar.bz2' | sha256sum -c - \
    && mkdir -p /opt/webos-sdk \
    && tar -xjf /tmp/webos-ndk.tar.bz2 -C /opt/webos-sdk --strip-components=1 \
    && rm /tmp/webos-ndk.tar.bz2 \
    && /opt/webos-sdk/relocate-sdk.sh
ENV WEBOS_SDK=/opt/webos-sdk \
    WEBOS_SYSROOT=/opt/webos-sdk/arm-webos-linux-gnueabi/sysroot \
    CC_arm_unknown_linux_gnueabi=/opt/webos-sdk/bin/arm-webos-linux-gnueabi-gcc.br_real \
    AR_arm_unknown_linux_gnueabi=/opt/webos-sdk/bin/arm-webos-linux-gnueabi-ar \
    CFLAGS_arm_unknown_linux_gnueabi=--sysroot=/opt/webos-sdk/arm-webos-linux-gnueabi/sysroot
