ARG TALOS_BROWSER_IMAGE
FROM ${TALOS_BROWSER_IMAGE}

USER root
COPY html /opt/miniwob/html
COPY episode-config.json /opt/miniwob/episode-config.json
RUN test -r /opt/talos/browser/runtime.json \
    && test -r /opt/talos/plugins/apilots-mcps/source.json
HEALTHCHECK --interval=1s CMD test -r /opt/miniwob/episode-config.json
WORKDIR /home/talos
