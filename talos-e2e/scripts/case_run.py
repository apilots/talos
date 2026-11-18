"""The before/after boundary used by every discovered scenario."""
import errno
import os
from pathlib import Path
import shutil
import signal
import socket as unix_socket
import subprocess
import sys
import time

from lifecycle import adopt_case_processes, cleanup, own_process, root, socket_directory
from result_report import write_json
from result_usage import collect_usage


def interrupted(signum, _frame):
    raise InterruptedError(signum)


def run(script, result_file=None):
    started = time.monotonic()
    case = os.environ['TALOS_E2E_CASE_ID']
    if Path(case).is_absolute() or '..' in Path(case).parts or not Path(case).parts:
        raise ValueError('Case ID must be a relative catalog path')
    directory = root() / 'cases' / case
    directory.mkdir(parents=True, exist_ok=False)
    temporary = socket_directory()
    (directory / 'tmp').symlink_to(temporary, target_is_directory=True)
    env = dict(os.environ, TALOS_E2E_TMP_ROOT=str(directory), TMPDIR=str(temporary) + '/',
               TMP=str(temporary), TEMP=str(temporary), TALOS_E2E_NATIVE_ROOT=str(directory / "native"), TMUX_TMPDIR=str(temporary))
    env.pop("TMUX", None)
    os.environ.update(TALOS_E2E_TMP_ROOT=str(directory))
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, interrupted)
    status = 1
    child = None
    cancelled = False
    try:
        child = own_process(subprocess.Popen(['bash', script], env=env, cwd=directory, start_new_session=True))
        status = child.wait()
    except InterruptedError as error:
        status = 128 + error.args[0]
        cancelled = True
    finally:
        # Ignore further signals while the bounded cleanup retires this case.
        for sig in (signal.SIGINT, signal.SIGTERM):
            signal.signal(sig, signal.SIG_IGN)
        seconds = round(time.monotonic() - started, 3)
        usage, coverage = [], ""
        if result_file:
            try:
                usage, coverage = collect_usage(directory, temporary)
            except OSError:
                coverage = "N/A（无法读取场景用量）"
        errors = []
        for socket in temporary.glob('tmux-*/default'):
            if socket.is_socket():
                try:
                    subprocess.run(['tmux', '-S', str(socket), 'kill-server'], capture_output=True,
                                   check=True, timeout=10)
                except subprocess.CalledProcessError as error:
                    # tmux exits on /quit but may leave its UNIX socket behind.
                    # Accept only a confirmed absent listener; a live server or an
                    # inaccessible socket still makes cleanup fail.
                    try:
                        with unix_socket.socket(unix_socket.AF_UNIX) as probe:
                            probe.settimeout(1)
                            probe.connect(str(socket))
                    except OSError as probe_error:
                        if probe_error.errno not in (errno.ENOENT, errno.ECONNREFUSED):
                            errors.append(f'Private tmux socket probe failed: {probe_error}')
                    else:
                        errors.append(f'Private tmux server cleanup failed: {error}')
                except (OSError, subprocess.SubprocessError) as error:
                    errors.append(f'Private tmux server cleanup failed: {error}')
        try:
            adopt_case_processes(directory)
            adopt_case_processes(temporary)
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
        if result_file:
            outcome = "CANCELLED" if cancelled else "PASS" if status == 0 else "FAIL"
            write_json(result_file, {"status": outcome, "exit_code": status,
                                     "seconds": seconds,
                                     "usage": usage, "coverage": coverage})
    return status if status >= 0 else 128 - status


if __name__ == '__main__':
    sys.exit(run(sys.argv[1], Path(sys.argv[2]) if len(sys.argv) > 2 else None))
