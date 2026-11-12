#!/bin/bash
set -euo pipefail

printf '%s\n' -1 > /logs/verifier/reward.txt
exec /opt/miniconda3/bin/python /tests/grade.py
