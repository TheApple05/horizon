"""Exercise the offline fork with synthetic credentials and isolated game data."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('resources', type=Path)
    args = parser.parse_args()
    binary, resources = args.binary.resolve(), args.resources.resolve()
    scratch = Path(tempfile.mkdtemp(prefix='offline-qa-', dir=Path('.local').resolve()))
    session = scratch / 'synthetic-session.json'
    contents = json.dumps(dict(name='PrivateCanary', uuid='0' * 31 + '1',
                               access_token='synthetic-not-a-real-token', user_type='msa'))
    session.write_text(contents)
    env = os.environ.copy()
    env.update(HORIZON_SESSION_FILE=str(session), HORIZON_PLAYER_NAME='PrivateCanary',
               HORIZON_ACCOUNT_SESSIONS=json.dumps([dict(name='PrivateCanary',
                   uuid='0' * 31 + '1', file=str(session))]),
               HORIZON_TELEMETRY_URL='http://127.0.0.1:9/never-send',
               HORIZON_LAUNCHER_NO_SELF_UPDATE='1', HORIZON_NO_AUDIO='1')
    for case, extra in [('environment', []),
                        ('explicit-session', ['--session-file', str(session), '--session-stdin'])]:
        folder = scratch / case
        folder.mkdir()
        for key in ['HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME']:
            env[key] = str(folder / key.lower())
        report = folder / 'report.json'
        command = [str(binary), '--resources', str(resources), '--game-dir', str(folder / 'game'),
                   '--home', env['HORIZON_HOME'], '--offscreen', '--world', 'OfflineSmoke',
                   '--creative', '--test-terrain', '--render-distance', '2', '--world-frames', '3',
                   '--smoke-report', str(report)] + extra
        # Leave stdin open and empty: the offline client must not wait for credentials.
        output_path = folder / 'output.txt'
        output_file = output_path.open('wb')
        process = subprocess.Popen(command, cwd=folder, env=env, stdin=subprocess.PIPE,
                                   stdout=output_file, stderr=subprocess.STDOUT)
        try:
            process.wait(timeout=300)
        except subprocess.TimeoutExpired:
            process.kill()
            raise AssertionError(f'{case}: startup timed out; evidence at {folder}')
        output_file.close()
        output = output_path.read_text(encoding='utf-8', errors='replace')
        process.stdin.close()
        assert process.returncode == 0, f'{case}: exit {process.returncode}; see {folder}'
        assert session.read_text() == contents, f'{case}: session file was modified'
        assert 'PrivateCanary' not in output and 'synthetic-not-a-real-token' not in output
        result = json.loads(report.read_text())
        assert result['error'] is None and result['player'] is not None, result
        logs = '\n'.join(p.read_text(errors='replace') for p in folder.rglob('latest.log'))
        assert 'TestPlayer' in logs, f'{case}: offline identity not observed'
        assert 'PrivateCanary' not in logs and 'synthetic-not-a-real-token' not in logs
        print(f'PASS {case}: offline world loaded, neutral identity, credential file untouched')
    print(f'Evidence: {scratch}')


if __name__ == '__main__':
    main()
