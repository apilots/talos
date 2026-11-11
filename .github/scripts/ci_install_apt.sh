#!/usr/bin/env bash
set -euo pipefail

# The hosted Ubuntu image reads this mirror list with the Ubuntu archive keyring.
if ! timeout 10 getent ahostsv4 azure.archive.ubuntu.com >/dev/null; then
  echo 'The default Ubuntu mirror did not resolve; using the MIT Ubuntu mirror.'
  printf '%s\n' 'https://mirrors.mit.edu/ubuntu/' | sudo tee /etc/apt/apt-mirrors.txt >/dev/null
fi

sudo apt-get update
sudo apt-get install -y --no-install-recommends "$@"
