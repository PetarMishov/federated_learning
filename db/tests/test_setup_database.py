import os
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
config_source = Path(sys.argv[1]) if len(sys.argv) > 1 else repo / '.env'
settings = {}
for line in config_source.read_text().splitlines():
    key, sep, value = line.partition('=')
    if sep and key.startswith('POSTGRES_'):
        settings[key] = value.strip().strip('\"\'')
test_db = 'fl_setup_test_' + uuid.uuid4().hex
env = dict(os.environ, PGPASSWORD=settings['POSTGRES_PASSWORD'], PGCONNECT_TIMEOUT='5')
connection = ['psql', '--host=127.0.0.1', '--port=' + settings['POSTGRES_PORT'], '--username=' + settings['POSTGRES_USER'], '--dbname=postgres', '--no-password', '--no-psqlrc', '--set=ON_ERROR_STOP=on']
with tempfile.TemporaryDirectory(prefix='fl-setup-test-') as directory:
    config = Path(directory) / 'test.env'
    config.write_text('\n'.join(f'{key}={test_db if key == "POSTGRES_DB" else value}' for key, value in settings.items()) + '\n')
    config.chmod(0o600)
    command = [str(repo / 'db/scripts/setup_db.sh'), '--env-file', str(config)]
    try:
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode:
            print(result.stderr.strip())
        assert result.returncode == 0, 'Setup must create a missing database before loading its schema'
        second = subprocess.run(command, capture_output=True, text=True)
        assert second.returncode != 0 and 'Setup requires an empty schema.' in second.stderr
        tables = subprocess.run(connection + ['--dbname=' + test_db, '--tuples-only', '--no-align', '--command=SELECT count(*) FROM pg_tables WHERE schemaname = \'public\';'], env=env, capture_output=True, text=True, check=True)
        assert tables.stdout.strip() == '14', tables.stdout
        print('PASS: missing database created with 14 tables; repeat setup refuses existing tables.')
    finally:
        subprocess.run(connection + ['--quiet', '--command=DROP DATABASE IF EXISTS "' + test_db + '" WITH (FORCE);'], env=env, capture_output=True, check=True)
