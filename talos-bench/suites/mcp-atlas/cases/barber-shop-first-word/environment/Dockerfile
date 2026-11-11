FROM python:3.12-slim-bookworm@sha256:df540bfd30bd380e89a349611ce42aa67bcb542d58c047706616eb6ba2f86a1b

RUN python3 -m pip install --no-cache-dir mcp==2.3.0
COPY bridge.py /opt/talos-atlas/bridge.py
WORKDIR /app
