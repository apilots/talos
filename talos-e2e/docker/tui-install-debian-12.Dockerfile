FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libssl3 \
        perl \
        python3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace/talos

CMD ["bash"]
