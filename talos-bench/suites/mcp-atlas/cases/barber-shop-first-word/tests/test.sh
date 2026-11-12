#!/bin/sh
set -eu
exec python3 /tests/grade.py /logs/artifacts/answer.txt /logs/verifier
