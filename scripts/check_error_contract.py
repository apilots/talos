"""Reject known regressions; behavioral contract tests remain authoritative."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def violations(path, text):
    errors = []
    # These narrow regression checks supplement typed APIs and behavioral tests.
    prose = re.sub(r"\btypeof\s+\w+\.message\b", "", text)
    message = r"\b(?:\w*[Ee]rror\w*|err|failure|wire)\.message\b"
    rendered = r"\b(?:apiErrorMessage|userVisibleErrorMessage|errorMessage)\([^;\n]*?\)"
    sentence = r'''(?:"[^"\n]+"|'[^'\n]+'|`[^`\n]+`)'''
    receiver = rf"(?:{message}|{rendered})"
    if any(re.search(pattern, prose) for pattern in [
        rf"{receiver}\s*[!=]={{1,2}}\s*{sentence}",
        rf"{sentence}\s*[!=]={{1,2}}\s*{receiver}",
        r"\.english_message\(\)\s*[!=]=",
        rf"{receiver}(?:\.\w+\(\))*\.(?:includes|contains|startsWith|starts_with|endsWith|ends_with|match|split|split_once)\(",
    ]):
        errors.append("Use error codes and typed parameters for decisions; messages are display-only.")
    if re.search(
        r"\b(?:AgentError|AgentErrorInfo|TalosMemoryErrorKind|TalosToolFailureDiagnostic)\b"
        r"|TalosError::(?:public|tool_failure)\s*\("
        r"|pub\s+enum\s+ErrorKind\b",
        text,
    ):
        errors.append("Do not reintroduce duplicate or visibility-based product errors.")
    if any(owner in path for owner in ["gateway/", "agent-kernel/", "agent-provider-kernel/", "action/mcp/"]):
        if re.search(r"(?:message|content)\s*:\s*(?:error|err)\.to_string\(\)", text):
            errors.append("Project ApiError.message, not diagnostic Display, into public/model output.")
    return errors


def main():
    errors = []
    for directory in ["talos-rs/crates", "talos-rs/apps", "talos-rs/bin"]:
        for file in (ROOT / directory).rglob("Cargo.toml"):
            if not re.search(r"(?m)^\[lints\]\s*\nworkspace\s*=\s*true\s*$", file.read_text()):
                errors.append(f"{file.relative_to(ROOT)}: Inherit workspace unused-Result enforcement.")
        for file in (ROOT / directory).rglob("*.rs"):
            if "tests" in file.parts or file.stem.endswith("tests"):
                continue
            relative = file.relative_to(ROOT).as_posix()
            errors.extend(f"{relative}: {message}" for message in violations(relative, file.read_text()))
    for directory, pattern in [
        ("talos-app/ts-sdk/packages", "*.ts"),
        ("talos-app/desktop/scripts", "*.mjs"),
    ]:
        for file in (ROOT / directory).rglob(pattern):
            if any(part in {"node_modules", "generated", "__tests__"} for part in file.parts) or ".test." in file.name:
                continue
            relative = file.relative_to(ROOT).as_posix()
            errors.extend(f"{relative}: {message}" for message in violations(relative, file.read_text()))
    if errors:
        raise SystemExit("\n".join(errors))
    print("Error contract repository checks passed.")


if __name__ == "__main__":
    main()
