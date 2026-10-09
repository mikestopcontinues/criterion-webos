FROM public.ecr.aws/docker/library/node:24.20.0-bookworm-slim@sha256:ba849c60be29959425b8734d57b8b4b7d56f98edd9504c9af091d5281095a71e
ARG TARGETARCH
RUN test "$TARGETARCH" = arm64
RUN apt-get update \
    && apt-get install -y --no-install-recommends binutils=2.40-2 \
    && rm -rf /var/lib/apt/lists/*
ADD https://nodejs.org/dist/v16.20.2/node-v16.20.2-linux-arm64.tar.gz /tmp/node16.tar.gz
RUN printf '%s  %s\n' 'b6945fcc9ad220386bb814bfae7137189fd17297f2959a744105e1bee006035a' '/tmp/node16.tar.gz' | sha256sum -c - \
    && mkdir -p /opt/node16 \
    && tar -xzf /tmp/node16.tar.gz -C /opt/node16 --strip-components=1 \
    && rm /tmp/node16.tar.gz
WORKDIR /workspace/tools/player-probe
