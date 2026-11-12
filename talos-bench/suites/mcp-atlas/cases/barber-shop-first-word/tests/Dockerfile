FROM python:3.12-slim-bookworm@sha256:df540bfd30bd380e89a349611ce42aa67bcb542d58c047706616eb6ba2f86a1b

ENV MPLBACKEND=Agg
RUN python3 -m pip install --no-cache-dir \
    aiohttp==3.13.3 matplotlib==3.10.8 nest-asyncio==1.6.0 pandas==2.3.3 \
    python-dotenv==1.2.4 tenacity==9.1.4 tqdm==4.67.3
ADD --checksum=sha256:fc3e9346e7d87b86c7d22151d4a95d4db4c61ec0365ebdc1faaf9dfe64dbb421 \
    https://raw.githubusercontent.com/scaleapi/mcp-atlas/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/services/scoring/score_claims.py \
    /opt/atlas/score_claims.py
COPY grade.py groundtruth.csv test.sh /tests/
WORKDIR /app
