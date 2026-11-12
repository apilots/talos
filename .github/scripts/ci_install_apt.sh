#!/usr/bin/env bash
set -euo pipefail

apt_options=(-o Acquire::Retries=0 -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15)

install_packages() {
  timeout --kill-after=10s 180s sudo apt-get "${apt_options[@]}" update --error-on=any &&
    sudo apt-get "${apt_options[@]}" install -y --no-install-recommends "$@"
}

if timeout 10 getent ahostsv4 azure.archive.ubuntu.com >/dev/null && install_packages "$@"; then
  exit 0
fi

# The hosted Ubuntu image reads this mirror list with the Ubuntu archive keyring.
echo 'The default Ubuntu package sources failed; retrying with the MIT Ubuntu mirror.'
printf '%s\n' 'https://mirrors.mit.edu/ubuntu/' | sudo tee /etc/apt/apt-mirrors.txt >/dev/null
install_packages "$@"
