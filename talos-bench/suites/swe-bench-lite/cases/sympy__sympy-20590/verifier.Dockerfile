ARG SWE_BENCH_AGENT_IMAGE
FROM ${SWE_BENCH_AGENT_IMAGE}

COPY --from=ghcr.io/astral-sh/uv:0.9.17@sha256:5cb6b54d2bc3fe2eb9a8483db958a0b9eebf9edff68adedb369df8e7b98711a2 /uv /usr/local/bin/uv
ARG SWEBENCH_VERSION
RUN --mount=type=cache,target=/root/.cache/uv \
    uv pip install --link-mode=copy --python /opt/miniconda3/bin/python "swebench==$SWEBENCH_VERSION"
# The official evaluator logs git show; avoid dumping the shallow root tree.
RUN git config --local log.showRoot false
# pip's --no-build-isolation option stores false in build_isolation.
ENV PIP_NO_BUILD_ISOLATION=0 PIP_NO_INDEX=1
COPY test.sh grade.py case.json /tests/
