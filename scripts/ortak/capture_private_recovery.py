#!/usr/bin/env python3
"""Capture a complete selected bundle only inside root's coordinated live quiescence lease."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import time
from uuid import uuid4

import backup_private_database as main_backup
import backup_private_honcho as honcho_backup
from check_private_recovery_gate import held_barrier, load_registry, root_pause_receipt
from prepare_private_recovery import CAPTURE_LIMITS, files, load_preparation, save, sha
from private_recovery_database_metadata import selected_extras, verified_content
import private_recovery_inventory as inventory
import private_recovery_obligations as obligations
import private_recovery_payloads as payload
import recovery_native_ingress as native_ingress
import private_recovery_workspace_capture as workspace_capture
import private_recovery_journal as selected_journal
import private_recovery_native_confidential as native_ciphertext
import private_recovery_scorer as scorer
import recovery_image_export as image_export

FORMAT = 'ortak-private-full-recovery-bundle/1'
FAILURE_CODES = frozenset(('capture_configuration_changed', 'full_capture_capacity_refused',
    'backup_failed_private_manifest_retained', 'honcho_backup_failed_private_evidence_retained',
    'honcho_restored_metadata_mismatch', 'restore_metadata_mismatch', 'database_component_unverified',
    'database_archive_changed', 'database_settings_or_sequence_restore_mismatch',
    'database_logical_rows_restore_mismatch', 'capture_obligation_generation_changed',
    'honcho_recovery_contract_changed', 'cold_store_pause_required', 'cold_store_unclean',
    'main_database_not_drained', 'main_schema_authority_changed', 'paused_configuration_generation_changed',
    'linux_lease_not_held', 'linux_lease_release_failed', 'linux_lease_process_failed',
    'drain_generation_changed', 'root_pause_receipt_changed', 'artifact_tree_changed', 'capture_image_missing',
    'cold_volume_reader_failed', 'command_failed', 'command_reported_diagnostics',
    'command_deadline_exceeded', 'operation_deadline_exceeded', 'command_output_limit_exceeded',
    'command_gzip_options_refused', 'image_export_metadata_refused'))


def failure_detail(error):
    """Only fixed typed codes and a scoped child receipt may enter a failed manifest."""
    code = str(error) if isinstance(error, main_backup.Refused) else None
    result = {'cause_code': 'capture_output_already_exists' if isinstance(error, FileExistsError)
        else code if code in FAILURE_CODES | workspace_capture.FAILURE_CODES | selected_journal.FAILURE_CODES
        else 'unclassified_capture_failure'}
    reader = getattr(error, 'reader_failure', None)
    if (isinstance(reader, dict) and set(reader) == {'kind', 'code', 'phase'}
        and reader['kind'] in ('redis', 'minio') and reader['code'] in payload.READER_CODES
        and reader['phase'] in payload.READER_PHASES):
        result['reader_failure'] = reader
    path = getattr(error, 'receipt_path', None)
    if not isinstance(path, Path):
        return result
    if not (path.name == 'manifest.json' and path.parent.parent in
            (inventory.STATE / 'backups', inventory.STATE / 'honcho-backups')
            and re.fullmatch(r'[0-9]{8}T[0-9]{6}Z_[0-9a-f]{32}', path.parent.name)):
        return {**result, 'child_receipt_refused': True}
    try:
        row, metadata = inventory.public_json(path.parent, path.name, maximum=1024**2)
        child_code = row.get('error_code')
        result.update(child_receipt=metadata,
            child_cause_code=child_code if child_code in FAILURE_CODES else 'unclassified_backup_failure')
        allowed = {'schema_sha256','schema_components','tables','logical_rows_sha256','owners','extensions',
            'role','server_version','migration_checksums','private_company','employee_states'}
        fields = row.get('different_fields', [])
        if isinstance(fields, list) and all(isinstance(key,str) and key in allowed for key in fields):
            result['different_fields'] = fields
    except Exception:
        result['child_receipt_refused'] = True
    return result


class Capture:
    """Each component uses an exact frozen source; no stop/resume or application start is implemented."""

    def __init__(self, output, registry):
        self.output, self.registry = output, registry
        self.prepared = load_preparation(Path(registry['preparation']))
        self.command = main_backup.Commands(main_backup.private_directory(output / 'commands', fresh=True))
        self.command.deadline = time.monotonic() + CAPTURE_LIMITS['whole_seconds']
        self.observation = self.prepared['observation']
        self.encrypted_extras = {}

    def current(self):
        """A changed public configuration or secret generation never silently enters this capture."""
        self.command.remaining()
        inventory.require(files() == self.observation['files'], 'capture_configuration_changed')

    def cold_stores(self):
        """Require root to have already stopped exact Redis/MinIO owners gracefully."""
        inspector = inventory.Inventory(main_backup.private_directory(self.output / 'cold-state', fresh=True))
        for name in ['redis', 'minio']:
            row = inspector.container(name)
            old = self.observation['containers'][name]
            inventory.require(not row['running'] and all(row[key] == old[key] for key in
                ['id', 'image', 'mounts', 'started_at', 'volume']), 'cold_store_pause_required')
            state = json.loads(inspector.run(self.command.docker('inspect', '--format',
                '{"exit_code":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}}}', row['id']), limit=1024))
            inventory.require(state['exit_code'] == 0 and not state['oom'], 'cold_store_unclean')

    def databases(self):
        """Both proven database helpers run within the same application-writer pause interval."""
        self.current()
        main = main_backup.backup(inventory.STATE)
        prepared = self.prepared
        class PausedHoncho(honcho_backup.HonchoCommands):
            def inspect(self):
                inspector = inventory.Inventory(self.root)
                inspector.commands = self
                database = inspector.container('honcho_postgres')
                api = inspector.container('honcho_api')
                old = prepared['observation']['containers']['honcho_api']
                inventory.require(not api['running'] and all(api[key] == old[key]
                    for key in ['id', 'image', 'mounts', 'started_at']), 'honcho_pause_binding_changed')
                saved = inventory.saved_honcho_selection()
                inventory.require(saved == prepared['observation']['honcho']['saved_selection'],
                                  'honcho_saved_binding_changed')
                self.container = database['id']
                return {'containers': {'honcho_postgres': database, 'honcho_api': api},
                        'selection': saved, 'last_live_setting_verified_at': prepared['observation']['observed_at'],
                        'current_api_stopped': True}
        honcho = honcho_backup.backup(inventory.STATE, commands_type=PausedHoncho)
        result = {}
        for name, directory, archive, source, command_type in [
            ('main', main, 'database.dump', 'ortak', main_backup.Commands),
            ('honcho', honcho, 'honcho.dump', honcho_backup.SOURCE, PausedHoncho),
        ]:
            receipt = json.loads((directory / 'manifest.json').read_text())
            inventory.require(receipt['status'] == 'verified', 'database_component_unverified')
            copied = payload.copy_file(directory / archive, self.output / (name + '.dump'), main_backup.MAX_DUMP)
            inventory.require(copied['sha256'] == receipt['archive_sha256'], 'database_archive_changed')
            receipt_copy = payload.copy_file(directory / 'manifest.json', self.output / (name + '-database.json'), 1024**2)
            commands = command_type(main_backup.private_directory(self.output / (name + '-settings'), fresh=True))
            commands.inspect()
            source_settings = selected_extras(commands, source, 'source-settings')
            restored_settings = selected_extras(commands, receipt['verification_database'], 'restored-settings')
            inventory.require(source_settings == restored_settings, 'database_settings_or_sequence_restore_mismatch')
            self.encrypted_extras[name + '-database-settings.json'] = json.dumps(source_settings, sort_keys=True).encode()
            source_content = verified_content(commands, source, receipt['verification_database'], receipt)
            result[name] = {**copied, 'receipt': receipt_copy, 'retained_verification_database': receipt['verification_database'],
                            'settings_sequences_verified': True, 'logical_rows_sha256': source_content}
            if name == 'main':
                evidence = obligations.observe(commands, source, receipt['expected'], inventory.COMPANY, drained=True)
                result[name]['recovery_obligations'] = obligations.verify_restore(commands,
                    receipt['verification_database'], receipt['restored'], inventory.COMPANY, evidence)
            else:
                result[name]['recovery_contract'] = obligations.verify_honcho(commands, source, receipt['expected'])
                inventory.require(obligations.verify_honcho(commands, receipt['verification_database'], receipt['restored'],
                    label='restored-reviewed-honcho-invariants')
                    == result[name]['recovery_contract'], 'honcho_recovery_contract_changed')
        self.current()
        return result

    def volumes(self):
        """Stream complete cold volume trees through read-only, pinned no-network readers."""
        result = {}
        for name in ['redis', 'minio']:
            self.command.remaining()
            volume = inventory.SERVICES[name][3]
            limit = CAPTURE_LIMITS[name + '_bytes']
            reader = 'ortak-recovery-reader-' + uuid4().hex
            image = self.observation['containers']['controller']['image']
            archive = self.output / (name + '.tar')
            args = self.command.docker('run', '--pull', 'never', '--name', reader,
                '--label', 'org.ortak.recovery_reader=' + self.output.name, '--network', 'none',
                '--read-only', '--user', '0:0', '--cap-drop', 'ALL', '--cap-add', 'DAC_OVERRIDE',
                '--security-opt', 'no-new-privileges', '--pids-limit', '16', '--memory', '128m',
                '--mount', 'type=volume,source=' + volume + ',target=/capture-source,readonly,volume-nocopy',
                '--entrypoint', '/usr/local/bin/python', image, '-u', '-c', payload.VOLUME_READER, str(limit))
            save(self.output / (name + '-reader-intent.json'), {'name': reader, 'source_volume': volume,
                'image': image, 'source_read_only': True, 'network': 'none', 'docker_socket': False})
            try:
                self.command.run(name + '-volume', args, output=archive, ceiling=limit)
            except main_backup.Refused:
                failure = main_backup.Refused('cold_volume_reader_failed')
                failure.reader_failure = payload.volume_reader_failure(
                    self.command.root / (name + '-volume.stderr'), name)
                raise failure from None
            result[name] = {'path': archive.name, 'bytes': archive.stat().st_size, 'sha256': main_backup.digest(archive),
                            'retained_reader': reader, 'complete_cold_tree': True,
                            'xattrs': 'exact_8byte_user.total_writes_and_user.total_deletes_only'}
        return result

    def journal(self):
        """Capture coherent SQLite plus any cold WAL/SHM evidence without application imports."""
        self.command.remaining()
        source_root=inventory.RUNTIME/'state'
        selected=selected_journal.selection(inventory.JOURNAL_VOLUME)
        protected=selected_journal.require_confidential_schema(
            self.observation.get('journal_confidential'),inventory.MAIN_SCHEMA_VERSION)
        storage={}
        if selected is not None:
            witness=getattr(self,'held_witness',None)
            selected_journal.require(witness is not None,'journal_lease_not_held')
            raw=selected_journal.receive(witness,self.output/'journal-raw.tar')
            source_root=self.output/'journal-raw'
            selected_journal.extract(self.output/'journal-raw.tar',source_root,raw['archive'],self.command)
            controller=self.observation['containers']['controller']
            storage={'raw_archive':raw,'source_storage':{'kind':'docker_volume','selection':selected,
                'source_uid':10001,'logical_path':str(inventory.RUNTIME/'state/journal.sqlite'),
                'controller_id':controller['id'],'controller_image':controller['image']}}
        result = payload.sqlite_backup(source_root / 'journal.sqlite', self.output / 'journal.sqlite', cold=True)
        companions = []
        for name in ['journal.sqlite-wal', 'journal.sqlite-shm']:
            source = source_root / name
            if source.exists():
                companions.append(payload.copy_file(source, self.output / ('cold-' + name), CAPTURE_LIMITS['sqlite_bytes']))
        result['cold_companions'] = companions
        if protected is not None:
            from restore_private_recovery import journal
            witness=getattr(self,'held_witness',None)
            selected_journal.require(witness is not None,'journal_lease_not_held')
            proof=journal(self.output/'journal.sqlite',confidential_reviewed=protected)
            selected_journal.require(proof['counters']==selected_journal.status(witness),
                'journal_archive_changed')
            result['confidential_selection']=protected
            result['confidential_proof']=proof
        return {**result,**storage}

    def public_artifacts(self):
        """Preserve exact public allowlists, immutable native directories, resume code and retained repos."""
        self.current()
        public = [(Path(row['path']), 'selected/' + row['path'].lstrip('/'))
                  for row in self.observation['files']['public']]
        public += [(Path(self.registry['preparation']), 'operation/preparation.json'),
                   (inventory.STATE / 'recovery-operations' / self.registry['operation_id'] / 'owners.json', 'operation/owners.json'),
                   (inventory.STATE / 'recovery-operations' / self.registry['operation_id'] / 'pause.json', 'operation/pause.json')]
        config = payload.archive_files(self.output / 'configuration.tar', public, CAPTURE_LIMITS['configuration_secret_bytes'])
        artifact_roots = sorted({Path(row['executable']).parent for row in self.observation['native_processes'].values()})
        entries = []
        limit = CAPTURE_LIMITS['native_artifacts_bytes']
        inspector = inventory.Inventory(main_backup.private_directory(self.output / 'native-client-inspect', fresh=True))
        for root in artifact_roots:
            entries += payload.tree_entries(root, 'native/' + root.name, limit)
        entries += native_ingress.capture_entries(inspector, self.observation['native_ingress'])
        resume = inventory.STATE / 'recovery-operations' / self.registry['operation_id'] / 'resume-code'
        entries += payload.tree_entries(resume, 'resume-code', limit)
        operator = inventory.STATE / 'recovery-operations' / self.registry['operation_id'] / 'operator-code'
        entries += payload.tree_entries(operator, 'operator-code', limit)
        repos = inventory.STATE / 'repos'
        if repos.exists(): entries += payload.tree_entries(repos, 'repos', limit)
        expected_tree = [(str(path), name, payload.fingerprint(path.lstat())) for path, name in entries]
        artifacts = payload.archive_files(self.output / 'native-and-repositories.tar', entries, limit)
        after = []
        for root in artifact_roots: after += payload.tree_entries(root, 'native/' + root.name, limit)
        after += native_ingress.capture_entries(inspector, self.observation['native_ingress'])
        after += payload.tree_entries(resume, 'resume-code', limit)
        after += payload.tree_entries(operator, 'operator-code', limit)
        if repos.exists(): after += payload.tree_entries(repos, 'repos', limit)
        inventory.require(expected_tree == [(str(path), name, payload.fingerprint(path.lstat())) for path, name in after],
                          'artifact_tree_changed')
        self.current()
        return {'configuration': config, 'native_and_repositories': artifacts}

    def native_confidential(self):
        """Capture the explicit bounded ciphertext component under the existing stopped-native barrier."""
        return native_ciphertext.capture(self)

    def images(self):
        """Export all selected immutable images; never pull a tag or save a live container."""
        images = self.prepared['plan']['images']
        for index, image in enumerate(images):
            current = self.command.run('image-' + str(index), self.command.docker('image', 'inspect', '--format', '{{.Id}}', image), ceiling=128).decode().strip()
            inventory.require(current == image, 'capture_image_missing')
        compressed=getattr(self,'gzip_images',False)
        limit=image_export.options(compressed,getattr(self,'image_output_limit',None),CAPTURE_LIMITS['image_exports_bytes'])
        target = self.output / ('images.tar.gz' if compressed else 'images.tar')
        kwargs={'gzip_output':True,'output_ceiling':limit} if compressed else {}
        receipt=self.command.run('images', self.command.docker('image', 'save', *images), output=target,
                         ceiling=CAPTURE_LIMITS['image_exports_bytes'],**kwargs)
        result={'path': target.name, 'bytes': target.stat().st_size, 'sha256': main_backup.digest(target), 'images': images}
        if compressed:
            inventory.require(receipt['bytes']==result['bytes'],'image_export_metadata_refused')
            result.update(format=image_export.FORMAT,compression='gzip',output_limit=limit,
                uncompressed_bytes=receipt['uncompressed_bytes'],uncompressed_sha256=receipt['uncompressed_sha256'])
        image_export.selection(result,CAPTURE_LIMITS['image_exports_bytes'])
        return result

    def workspace_files(self, witness):
        """Capture selected inputs, immutable copies and the reader only while the live barrier is held."""
        return workspace_capture.capture_workspace(self.observation['workspace_selection'],
            main_backup.private_directory(self.output / 'workspace-files',fresh=True),witness,self.command)

    def secrets(self, components):
        """Only this final gated capture action opens the selected secret/OAuth files."""
        self.current()
        scorer.require_held(getattr(self,'held_witness',None),self.observation.get('scorer_owner'))
        keys = main_backup.private_directory(inventory.STATE / 'recovery-keys')
        aad = {'format': FORMAT, 'operation_id': self.output.name, 'components_sha256': sha(components),
               'secret_metadata_sha256': sha(self.observation['files']['secret_metadata_only'])}
        result = payload.secret_envelope(self.output / 'secrets.aesgcm', keys / (self.output.name + '.key'),
            self.observation['files']['secret_metadata_only'], aad, self.encrypted_extras)
        self.current()
        scorer.require_held(getattr(self,'held_witness',None),self.observation.get('scorer_owner'))
        return result


def capture(owners, pause_receipt, *, backend_type=Capture, barrier=held_barrier,
            gzip_images=False, image_output_limit=None):
    """Seal only after every component and both barrier release acknowledgements succeed."""
    image_limit=image_export.options(gzip_images,image_output_limit,CAPTURE_LIMITS['image_exports_bytes'])
    registry = load_registry(owners)
    pause = root_pause_receipt(pause_receipt, registry)
    parent = main_backup.private_directory(inventory.STATE / 'recovery-bundles')
    output = main_backup.private_directory(parent / uuid4().hex, fresh=True)
    manifest = {'format': FORMAT, 'operation_id': output.name, 'status': 'started',
        'owners_sha256': registry['registry_sha256'], 'pause_receipt': pause,
        'started_at': datetime.now(timezone.utc).isoformat(), 'components': {},
        'source_service_actions': False, 'source_resume_required_from_root': True,
        'full_restore_executed': False, 'independent_host_verified': False, 'automatic_activation': False}
    if gzip_images:
        manifest['image_export_options']={'format':image_export.FORMAT,'compression':'gzip',
            'output_limit':image_limit,'uncompressed_limit':CAPTURE_LIMITS['image_exports_bytes']}
    save(output / 'intent.json', manifest)
    phase = 'capacity_and_registry'
    try:
        capacity=sum(value for key,value in CAPTURE_LIMITS.items() if key.endswith('_bytes'))
        capacity=capacity-CAPTURE_LIMITS['image_exports_bytes']+image_limit
        inventory.require(shutil.disk_usage(output).free >= capacity,
                          'full_capture_capacity_refused')
        backend = backend_type(output, registry)
        if gzip_images:backend.gzip_images,backend.image_output_limit=True,image_limit
        if backend.observation.get('workspace_selection') is not None:
            inventory.require(shutil.disk_usage(output).free >= capacity + workspace_capture.files.MAX_ARCHIVE + workspace_capture.files.MAX_MANIFEST,
                'full_capture_capacity_refused')
        gate_dir = main_backup.private_directory(output / 'gate', fresh=True)
        phase = 'held_barrier_admission'
        with barrier(gate_dir, registry, pause_receipt=pause_receipt) as witness:
            backend.held_witness=witness
            manifest['barrier'] = witness
            if backend.observation.get('scorer_owner') is not None:
                manifest['components']['scorer']=scorer.require_held(witness,backend.observation['scorer_owner'])
            phase = 'cold_stores'
            backend.cold_stores()
            phases=['databases', 'volumes', 'journal']
            if backend.observation.get('native_confidential') is not None: phases.append('native_confidential')
            if backend.observation.get('workspace_selection') is not None: phases.append('workspace_files')
            phases += ['public_artifacts', 'images']
            for name in phases:
                phase = name
                save(output / (name + '-intent.json'), {'phase': name, 'status': 'started'})
                manifest['components'][name] = (backend.workspace_files(witness) if name=='workspace_files'
                    else getattr(backend, name)())
                if name == 'databases':
                    inventory.require(manifest['components'][name]['main']['recovery_obligations']['evidence']
                        == witness['databases']['recovery_obligations'], 'capture_obligation_generation_changed')
                save(output / (name + '-complete.json'), {'phase': name, 'status': 'captured',
                     'component': manifest['components'][name]})
            phase = 'secrets'
            manifest['secrets'] = backend.secrets(manifest['components'])
            backend.current()
            phase = 'held_barrier_release'
        phase = 'seal'
        manifest['status'] = 'captured'
        manifest['completed_at'] = datetime.now(timezone.utc).isoformat()
        manifest['manifest_sha256'] = sha(manifest)
        save(output / 'manifest.json', manifest)
        return output
    except Exception as error:
        manifest['status'] = 'failed'
        manifest.pop('manifest_sha256', None)
        manifest['error_code'] = 'full_capture_failed_private_evidence_retained'
        manifest['exception_type'] = type(error).__name__
        manifest['failed_phase'] = phase
        manifest.update(failure_detail(error))
        # A seal write/fsync may leave a complete-looking manifest on disk.
        # Preserve it, publish failure separately, and make restore refuse the
        # marker even if that earlier manifest's hash is valid. Never retry an
        # occupied O_EXCL destination or silently report a failure-write error.
        save(output / 'failure.json', manifest)
        if not os.path.lexists(output / 'manifest.json'):
            save(output / 'manifest.json', manifest)
        raise main_backup.Refused('full_capture_failed_private_evidence_retained') from None


def main():
    """The operator must supply the exact final owners and actual root pause receipt."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--owners', type=Path, required=True)
    parser.add_argument('--pause-receipt', type=Path, required=True)
    parser.add_argument('--gzip-images', action='store_true', help='Stream images directly into one bounded gzip archive.')
    parser.add_argument('--image-output-limit',type=int,
        help='Optional physical gzip byte cap; uncompressed image cap remains unchanged. Requires --gzip-images.')
    args = parser.parse_args()
    output = capture(args.owners, args.pause_receipt,gzip_images=args.gzip_images,image_output_limit=args.image_output_limit)
    print(json.dumps({'status': 'captured', 'manifest': str(output / 'manifest.json'),
                     'source_resume_required_from_root': True, 'full_restore_executed': False}))


if __name__ == '__main__':
    try:
        main()
    except (main_backup.Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Full capture refused; private partial artifacts retained. Source resume remains under root control.') from None
