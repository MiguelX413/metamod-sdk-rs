import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


@unittest.skipUnless(os.name == "posix" and shutil.which("bash") and shutil.which("timeout"),
                     "requires Bash and timeout on POSIX")
class UbuntuVersionProbeTests(unittest.TestCase):
    def test_manual_launchers_supply_library_path_without_wrapper_help(self):
        script = Path(__file__).with_name("ricochet_audit_ubuntu.sh").read_text()
        start = script.index('cat > "$RUNROOT/start-rehlds.sh"')
        end = script.index('cat > "$RUNROOT/README.txt"', start)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            installs = root / "installs"
            env = {**os.environ, "INSTALLS": str(installs), "RUNROOT": str(root),
                   "MAP_1": "rc_arena", "LD_LIBRARY_PATH": "/wrong-path"}
            for engine in ("hlds", "rehlds"):
                stage = installs / f"linux-{engine}"
                stage.mkdir(parents=True)
                wrapper = stage / "hlds_run"
                wrapper.write_text('#!/bin/sh\nprintf "%s\\n" "$LD_LIBRARY_PATH"\n')
                wrapper.chmod(0o755)
            subprocess.run(["bash", "-ec", script[start:end]], env=env, check=True)
            for engine in ("hlds", "rehlds"):
                output = subprocess.check_output(["bash", str(root / f"start-{engine}.sh")],
                                                 cwd=root, env=env, text=True)
                self.assertEqual(output.strip(), str(installs / f"linux-{engine}"))

    def test_probes_use_their_own_staged_library_paths(self):
        checkout = Path(__file__).resolve().parents[2]
        script = (checkout / "verify/audit/ricochet_audit_ubuntu.sh").read_text()
        start = script.index('if [[ -z $REHLDS_CONSOLE_VERSION ]]; then\n  say "Probing')
        end = script.index('say "Generating a matrix', start)
        probes = script[start:end]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            installs = root / "installs"
            for name, banner in (
                ("linux-hlds", "Exe version: 1.1.2.7"),
                ("linux-rehlds", "ReHLDS version: 3.15.0.896-dev"),
            ):
                stage = installs / name
                stage.mkdir(parents=True)
                stub = stage / "hlds_linux"
                stub.write_text(
                    '#!/bin/sh\n'
                    'printf "loader path: %s\\n" "${LD_LIBRARY_PATH:-}"\n'
                    '[ "${LD_LIBRARY_PATH:-}" = "$PWD" ] || exit 1\n'
                    'if [ "${FAIL_VALVE:-}" = 1 ] && [ "${PWD##*/}" = linux-hlds ]; then\n'
                    '  echo "fixture loader failure" >&2\n'
                    '  exit 1\n'
                    'fi\n'
                    f'echo "{banner}"\n'
                )
                stub.chmod(0o755)
            for fail in (False, True):
                with self.subTest(fail_valve=fail):
                    result = subprocess.run(
                        ["bash", "-ec", "say() { :; }\n" + probes +
                         'printf "%s\\n" "$VALVE_VERSION" "$REHLDS_CONSOLE_VERSION"'],
                        env={**os.environ, "INSTALLS": str(installs), "RUNROOT": str(root),
                             "REPO": str(checkout), "MAP_1": "rc_arena", "VALVE_VERSION": "",
                             "REHLDS_CONSOLE_VERSION": "", "LD_LIBRARY_PATH": "/wrong-path",
                             "FAIL_VALVE": str(int(fail))},
                        stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True,
                    )
                    expected_valve = "unverified-steam-legacy" if fail else "1.1.2.7"
                    self.assertEqual(result.stdout.splitlines(), [expected_valve, "3.15.0.896-dev"])
                    valve_log = (root / "valve-version-probe.log").read_text()
                    rehlds_log = (root / "rehlds-version-probe.log").read_text()
                    self.assertIn(f"loader path: {installs / 'linux-hlds'}\n", valve_log)
                    self.assertIn(f"loader path: {installs / 'linux-rehlds'}\n", rehlds_log)
                    if fail:
                        self.assertIn("fixture loader failure", valve_log)
