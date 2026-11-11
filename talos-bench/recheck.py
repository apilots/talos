"""Recheck an archived Terminal-Bench submission."""

from pathlib import Path
from runpy import run_path


if __name__ == "__main__":
    suite = Path(__file__).resolve().parent / "suites/terminal-bench"
    run_path(str(suite / "terminal_recheck.py"), run_name="__main__")
