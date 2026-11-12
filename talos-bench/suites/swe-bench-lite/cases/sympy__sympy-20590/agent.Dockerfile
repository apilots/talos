ARG SWE_BENCH_ENV
FROM --platform=linux/arm64 ${SWE_BENCH_ENV}

WORKDIR /app
ARG TASK_BASE_COMMIT
RUN set -eux; \
    git init /app; \
    git -C /app fetch --depth=1 https://github.com/sympy/sympy.git "$TASK_BASE_COMMIT"; \
    git -C /app checkout --detach FETCH_HEAD; \
    test "$(git -C /app rev-parse HEAD)" = "$TASK_BASE_COMMIT"; \
    test "$(git -C /app rev-list --all --count)" = 1; \
    rmdir /testbed; \
    ln -s /app /testbed; \
    /opt/miniconda3/envs/testbed/bin/python -m pip install -e /app; \
    test -z "$(git -C /app status --porcelain)"; \
    git config --system --add safe.directory /app; \
    chmod -R a+rwX /app
