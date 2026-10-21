"""The before/after boundary used by every discovered scenario."""
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys

from lifecycle import adopt_case_processes, cleanup, own_process, root, socket_directory


def interrupted(signum, _frame):
    raise InterruptedError(signum)


def run(script):
    case = os.environ['TALOS_E2E_CASE_ID']
    if Path(case).is_absolute() or '..' in Path(case).parts or not Path(case).parts:
        raise ValueError('Case ID must be a relative catalog path')
    directory = root() / 'cases' / case
    directory.mkdir(parents=True, exist_ok=False)
    temporary = directory / 'tmp'
    temporary.mkdir()
    tmux_root = socket_directory()
    env = dict(os.environ, TALOS_E2E_TMP_ROOT=str(directory), TMPDIR=str(temporary) + '/',
               TMP=str(temporary), TEMP=str(temporary), TALOS_E2E_NATIVE_ROOT=str(directory / "native"), TMUX_TMPDIR=str(tmux_root))
    env.pop("TMUX", None)
    os.environ.update(TALOS_E2E_TMP_ROOT=str(directory))
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, interrupted)
    status = 1
    child = None
    try:
        child = own_process(subprocess.Popen(['bash', script], env=env, cwd=directory, start_new_session=True))
        status = child.wait()
    except InterruptedError as error:
        status = 128 + error.args[0]
    finally:
        # Ignore further signals while the bounded cleanup retires this case.
        for sig in (signal.SIGINT, signal.SIGTERM):
            signal.signal(sig, signal.SIG_IGN)
        errors = []
        for socket in tmux_root.glob('tmux-*/default'):
            if socket.is_socket():
                try:
                    subprocess.run(['tmux', '-S', str(socket), 'kill-server'], capture_output=True,
                                   check=True, timeout=10)
                except (OSError, subprocess.SubprocessError) as error:
                    errors.append(f'Private tmux server cleanup failed: {error}')
        try:
            adopt_case_processes(directory)
        except (OSError, subprocess.SubprocessError) as error:
            errors.append(f'Case process discovery failed: {error}')
        errors.extend(cleanup(case))
        if child is not None:
            try:
                child.wait(timeout=1)
            except subprocess.TimeoutExpired:
                errors.append('Case process did not terminate')
        if not errors:
            try:
                shutil.rmtree(directory)
            except OSError as error:
                errors.append(str(error))
        for error in errors:
            print(error, file=sys.stderr)
        if errors and status == 0:
            status = 1
    return status if status >= 0 else 128 - status


if __name__ == '__main__':
    sys.exit(run(sys.argv[1]))
