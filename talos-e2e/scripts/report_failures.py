"""Print bounded, redacted E2E failure evidence for CI."""
import os
from pathlib import Path
import re
import sys


def redact(text, env):
    secrets = [value for key, value in env.items()
               if re.search(r"KEY|TOKEN|PASSWORD|SECRET", key) and len(value) >= 8]
    for secret in sorted(secrets, key=len, reverse=True):
        text = text.replace(secret, "<redacted>")
    text = re.sub(r"(?i)(bearer\s+)\S+", r"\1<redacted>", text)
    text = re.sub(r"eyJ[\w-]+\.[\w-]+\.[\w-]+", "<redacted>", text)
    return re.sub(r'''(?i)(["']?(?:\w*(?:token|password|secret|api_?key|credentials?)\w*)["']?\s*[:=]\s*)(["'])(.*?)\2''',
                  r"\1\2<redacted>\2", text)


def failures(root, env):
    for report in sorted(root.glob("e2e_report_*.md")):
        for section in report.read_text(errors="replace").split("### 场景 ")[1:]:
            if "- 状态: FAIL\n" not in section:
                continue
            name = section.splitlines()[0]
            log = section.split("```text\n", 1)[-1].rsplit("```", 1)[0]
            yield redact("Failed scenario: " + name + "\n" + "\n".join(log.splitlines()[-80:]), env)
    for report in sorted(root.glob("preflight_*.log")):
        text = report.read_text(errors="replace")
        if "BLOCKED " in text or "Runner error:" in text:
            yield redact("Prerequisite failure: " + report.name + "\n" + text, env)


if __name__ == "__main__":
    for item in failures(Path(sys.argv[1] if len(sys.argv) > 1 else "talos-e2e/reports"), os.environ):
        print(item)
