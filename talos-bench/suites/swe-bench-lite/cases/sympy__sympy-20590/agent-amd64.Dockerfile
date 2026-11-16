ARG SWE_BENCH_ENV
FROM ${SWE_BENCH_ENV}

ARG TASK_BASE_COMMIT
RUN set -eux; \
    test "$(git -C /testbed rev-parse HEAD)" = "$TASK_BASE_COMMIT"; \
    test -z "$(git -C /testbed status --porcelain)"; \
    ln -s /testbed /app
WORKDIR /app
