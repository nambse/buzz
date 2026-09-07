"""Production Capture.databases + real exclusive command files; synthetic DB transport only."""

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import capture_private_recovery as subject
from backup_private_database import Commands, digest, private_directory


class DatabasePhaseTests(unittest.TestCase):
    def test_entire_database_phase_preserves_distinct_source_restore_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(); root.chmod(0o700)
            main_tables = {'public.' + table: 0 for table in subject.obligations.table_keys(69)}
            honcho_tables = {'public.' + table: 0 for table in
                (*subject.obligations.HONCHO_BASE, *subject.obligations.HONCHO_REVIEWED)}
            main_metadata = {'tables': main_tables, 'migration_checksums': [[v, 'a' * 96, True] for v in range(1, 70)]}
            honcho_metadata = {'tables': honcho_tables}
            documents = {
                'main-content': dict.fromkeys(main_tables, 'a' * 64),
                'honcho-content': dict.fromkeys(honcho_tables, 'b' * 64),
                'obligations': {'counters': dict.fromkeys(subject.obligations.counters(69), 0),
                    'evidence': {'schema_version': 69, 'company_id': subject.inventory.COMPANY,
                        'tables': {table: [] for table in subject.obligations.table_keys(69)}}},
                'honcho-invariants': dict.fromkeys(subject.obligations.HONCHO_COUNTERS, 0),
            }
            # A real bounded child consumes the exact production SQL via stdin.
            # Commands.run, create_file, O_EXCL, archive copy and all validation
            # functions remain production code. No Docker/PG/provider is used.
            transport = root / 'synthetic-psql.py'
            transport.write_text('import json,sys\n'
                + 'documents=' + repr(documents) + '\n'
                + "sql=sys.stdin.read(); main=sys.argv[1].startswith('ortak') and not sys.argv[1].startswith('ortak_honcho')\n"
                + "role='ortak' if main else 'ortak_honcho'\n"
                + "if 'pg_db_role_setting' in sql: value={'role':{'name':role},'database':{'owner':role,'tablespace':'pg_default'},'settings':None,'sequences':None}\n"
                + "elif 'schema_version' in sql: value=documents['obligations']\n"
                + "elif 'content_without_matching_header' in sql: value=documents['honcho-invariants']\n"
                + "else: value=documents['main-content' if main else 'honcho-content']\n"
                + "print(json.dumps(value))\n")
            archives = {}
            for name, filename, metadata, target in [
                ('main', 'database.dump', main_metadata, 'ortak_verify_' + 'a' * 32),
                ('honcho', 'honcho.dump', honcho_metadata, 'ortak_honcho_verify_' + 'b' * 32),
            ]:
                directory = private_directory(root / (name + '-backup'), fresh=True)
                archive = directory / filename; archive.write_bytes((name + '-fixture-archive').encode()); archive.chmod(0o600)
                subject.save(directory / 'manifest.json', {'status': 'verified', 'archive_sha256': digest(archive),
                    'verification_database': target, 'expected': metadata, 'restored': metadata})
                archives[name] = directory

            class LocalCommands(Commands):
                def inspect(self): self.container = 'fixture'
                def psql(self, database): return [sys.executable, str(transport), database]

            selected_api = {'running': False, 'id': 'fixture-api', 'image': 'fixture-image',
                'mounts': [], 'started_at': 'fixture-start'}

            class Inventory:
                def __init__(self, output): self.root = output
                def container(self, name):
                    return {'id': 'fixture-db'} if name == 'honcho_postgres' else selected_api

            output = private_directory(root / 'capture', fresh=True)
            backend = subject.Capture.__new__(subject.Capture)
            backend.output = output
            backend.observation = {'files': {'fixture': True}}
            backend.prepared = {'observation': {'containers': {'honcho_api': selected_api},
                'honcho': {'saved_selection': {'fixture': True}}, 'observed_at': 'fixture-at'}}
            backend.command = Commands(private_directory(output / 'commands', fresh=True))
            backend.encrypted_extras = {}
            before = {str(p): p.read_bytes() for directory in archives.values() for p in directory.iterdir()}
            with patch.object(subject, 'files', return_value={'fixture': True}), \
                patch.object(subject.main_backup, 'backup', return_value=archives['main']), \
                patch.object(subject.honcho_backup, 'backup', return_value=archives['honcho']), \
                patch.object(subject.main_backup, 'Commands', LocalCommands), \
                patch.object(subject.honcho_backup, 'HonchoCommands', LocalCommands), \
                patch.object(subject.inventory, 'Inventory', Inventory), \
                patch.object(subject.inventory, 'saved_honcho_selection', return_value={'fixture': True}):
                result = backend.databases()
                self.assertEqual(set(result), {'main', 'honcho'})
                self.assertFalse(result['main']['recovery_obligations']['automatic_activation'])
                self.assertEqual(result['honcho']['recovery_contract']['reviewed_wire_family'], 'reviewed-project/1')
                self.assertTrue(all(row['settings_sequences_verified'] for row in result.values()))
                self.assertEqual(set(backend.encrypted_extras), {'main-database-settings.json', 'honcho-database-settings.json'})
                for name, labels in [('main', ('recovery-obligations', 'restored-recovery-obligations')),
                    ('honcho', ('reviewed-honcho-invariants', 'restored-reviewed-honcho-invariants'))]:
                    directory = output / (name + '-settings')
                    self.assertEqual(len(list(directory.glob('*.sql'))), 6)
                    for label in labels:
                        self.assertIn('READ ONLY', (directory / (label + '.sql')).read_text())
                        self.assertEqual((directory / (label + '.sql')).stat().st_mode & 0o777, 0o600)
                        self.assertEqual((directory / (label + '.stderr')).read_bytes(), b'')
                # Reusing a destination still refuses; no overwrite/append fallback.
                with self.assertRaises(FileExistsError): backend.databases()
            self.assertEqual(before, {str(p): p.read_bytes() for directory in archives.values() for p in directory.iterdir()})


if __name__ == '__main__': unittest.main()
