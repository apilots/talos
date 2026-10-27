#!/usr/bin/env bash
# Install one runtime contract for embedded and standalone container Workers.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive TZ=Etc/UTC

apt-get update
apt-get install -y --no-install-recommends \
    bash bubblewrap ca-certificates curl git libsqlite3-0 libssl3t64 ripgrep tini tzdata \
    locales ncurses-term openssh-client dnsutils iproute2 iputils-ping netcat-openbsd traceroute \
    procps psmisc lsof jq less vim-tiny rsync wget unzip zip tar gzip openssl \
    python3 python3-venv xvfb xauth x11-utils openbox x11vnc feh tint2 xterm pcmanfm desktop-file-utils librsvg2-bin \
    fonts-noto-cjk fonts-noto-color-emoji libasound2t64 libatk-bridge2.0-0t64 libatk1.0-0t64 \
    libatspi2.0-0t64 libcairo2 libcups2t64 libdbus-1-3 libdrm2 libgbm1 libglib2.0-0t64 \
    libnspr4 libnss3 libpango-1.0-0 libx11-6 libx11-xcb1 libxcb1 libxcomposite1 libxdamage1 \
    libxext6 libxfixes3 libxkbcommon0 libxrandr2
rm -rf /var/lib/apt/lists/*
locale-gen en_US.UTF-8

python3 -m venv /opt/talos/browser/venv
/opt/talos/browser/venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: \
    -r /opt/talos/plugins/apilots-mcps/mcp/browser/requirements.lock
/opt/talos/browser/venv/bin/python /opt/talos/browser/environment.py
rsvg-convert /opt/talos/browser/desktop/wallpaper.svg -o /opt/talos/browser/desktop/wallpaper.png
for icon in browser terminal files; do
    rsvg-convert -w 64 -h 64 "/opt/talos/browser/desktop/$icon.svg" -o "/opt/talos/browser/desktop/$icon.png"
done
cp /opt/talos/browser/desktop/view-file.desktop /usr/share/applications/talos-view-file.desktop
cp /opt/talos/browser/desktop/mimeapps.list /etc/xdg/mimeapps.list
update-desktop-database /usr/share/applications

groupadd --gid 10001 talos
useradd --uid 10001 --gid talos --home-dir /home/talos --create-home --shell /bin/bash talos
chmod 0755 /home/talos
install -d -o talos -g talos /opt/talos/bin /opt/talos/conf /home/talos/.talos
