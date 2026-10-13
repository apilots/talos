#!/usr/bin/env bash
# Group-owned S3 fixture. The caller installs its EXIT trap before starting it.

start_s3_gateway() {
  local platform archive digest
  platform="$(uname -s)_$(uname -m)"
  case "${platform}" in
    Darwin_arm64) digest=4953096f65a9c0d62ab184fb6b2ba7c2435229205cf00a56cb62cd4bf6b216ca ;;
    Darwin_x86_64) digest=104bd978e80ef0173554cfd8630ccc5bac24b00da440510b60365a3b6b6ab134 ;;
    Linux_aarch64) platform=Linux_arm64; digest=b34051d33f5a9c457f790896acb7bd7d7e15ad8d92efb70616b924f37e401910 ;;
    Linux_x86_64) digest=2ba2c734d10d2c4e651d03182cb4b246656bc735a2f282db7b0b73fba6073467 ;;
    *) echo "Unsupported S3 fixture platform: ${platform}" >&2; return 1 ;;
  esac
  archive="${TMP_DIR}/gateway.tar.gz"
  curl --proto '=https' --tlsv1.2 -fsSL \
    "https://github.com/versity/versitygw/releases/download/v1.8.0/versitygw_v1.8.0_${platform}.tar.gz" -o "${archive}"
  python3 - "${archive}" "${digest}" <<'PY'
import hashlib
import sys
from pathlib import Path
if hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest() != sys.argv[2]:
    raise SystemExit('S3 fixture archive checksum mismatch')
PY
  mkdir -p "${TMP_DIR}/bin" "${TMP_DIR}/objects"
  tar -xzf "${archive}" --strip-components=1 -C "${TMP_DIR}/bin"
  S3_PORT="$(python3 - <<'PY'
import socket
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    print(listener.getsockname()[1])
PY
)"
  S3_ENDPOINT="http://127.0.0.1:${S3_PORT}"
  S3_ACCESS_KEY=talos-files-e2e
  S3_SECRET_KEY=talos-files-e2e-secret
  ROOT_ACCESS_KEY="${S3_ACCESS_KEY}" ROOT_SECRET_KEY="${S3_SECRET_KEY}" \
    "${TMP_DIR}/bin/versitygw" --port "127.0.0.1:${S3_PORT}" posix "${TMP_DIR}/objects" \
    >"${TMP_DIR}/gateway.log" 2>&1 &
  S3_PID=$!
  for _ in $(seq 1 120); do
    kill -0 "${S3_PID}" 2>/dev/null || { cat "${TMP_DIR}/gateway.log" >&2; return 1; }
    if s3_request GET '' >"${TMP_DIR}/buckets.xml" 2>/dev/null; then
      s3_request PUT talos-files >/dev/null
      echo 'CASE s3_gateway_start PASS version=1.8.0'
      return 0
    fi
    sleep 0.25
  done
  cat "${TMP_DIR}/gateway.log" >&2
  return 1
}

s3_request() {
  curl -fsS --aws-sigv4 'aws:amz:us-east-1:s3' \
    --user "${S3_ACCESS_KEY}:${S3_SECRET_KEY}" -X "$1" "${S3_ENDPOINT}/$2"
}

stop_s3_gateway() {
  if [[ -n "${S3_PID:-}" ]]; then
    kill "${S3_PID}" 2>/dev/null || true
    wait "${S3_PID}" 2>/dev/null || true
    S3_PID=''
  fi
}
