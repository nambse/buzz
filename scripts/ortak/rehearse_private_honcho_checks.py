#!/usr/bin/env python3
"""Rehearse exact CHECK repair on a new offline store using one retained Honcho archive."""

import argparse
import hashlib
import json
from pathlib import Path
import secrets
from uuid import uuid4

from backup_private_database import Refused, digest, private_binary, private_directory
from backup_private_honcho import HonchoCommands, SOURCE
from prepare_private_recovery import save, sha
from private_recovery_database_metadata import selected_extras
from private_recovery_offline_stores import Postgres
from private_restore_honcho_checks import expected, source_checks
import private_recovery_inventory as inventory

ARCHIVE_ROOT = inventory.STATE / 'honcho-backups/20260905T224142Z_012882736cc74b9fb5fabb87edc27cea'
ARCHIVE_SHA = 'f1f890c2172bd549887acc0988b79d1c980cd53d0b0b32b6b494b13b91c29638'
SOURCE_SCHEMA = 'b017466a67f4ed13f7b4151ff9dd8a4daa680781b946bc2657a8b27cb5a97209'
IMAGE = 'sha256:cf134a767f474095eeba57e0117be8e568e011a63f33fbf252f14c9b760f8e6f'


def rehearse():
    """Use production target creation/restore/repair and retain every generated resource."""
    operation=uuid4().hex
    root=private_directory(inventory.EVIDENCE/('g-honcho-check-roundtrip-'+operation),fresh=True)
    receipt={'status':'started','operation':operation,'source_mutations':False,
        'failed_target_mutations':False,'provider_requests':False,'source_archive':str(ARCHIVE_ROOT/'honcho.dump'),
        'network':'none','published_ports':[],'docker_socket':False,'source_service_mounts':False}
    save(root/'intent.json',receipt)
    target=None
    try:
        manifest,manifest_metadata=inventory.public_json(ARCHIVE_ROOT,'manifest.json',maximum=1024**2)
        inventory.require(manifest['status']=='failed' and manifest['error_code']=='honcho_restored_metadata_mismatch'
            and manifest['different_fields']==['schema_sha256'] and manifest['archive_sha256']==ARCHIVE_SHA
            and manifest['expected']['schema_sha256']==SOURCE_SCHEMA,'selected_failed_archive_refused')
        archive=ARCHIVE_ROOT/'honcho.dump'
        archive_metadata=inventory.file_metadata(ARCHIVE_ROOT,'honcho.dump')
        inventory.require(archive_metadata['bytes']==manifest['archive_bytes'] and digest(archive)==ARCHIVE_SHA,'selected_archive_changed')
        receipt['archive_manifest']=manifest_metadata
        source=HonchoCommands(private_directory(root/'source-read-only',fresh=True))
        inspector=inventory.Inventory(private_directory(root/'source-owner',fresh=True))
        source.container=inspector.container('honcho_postgres')['id']
        with source.snapshot() as snapshot:
            selected=source_checks(source,SOURCE,snapshot)
            current=source.metadata(SOURCE,'current-source',snapshot)
        inventory.require(selected==expected() and current==manifest['expected'],'source_archive_generation_changed')
        settings=selected_extras(source,SOURCE,'source-settings')
        password=root/'fixture-password'
        with private_binary(password) as stream: stream.write(secrets.token_hex(32).encode())
        target=Postgres(private_directory(root/'target',fresh=True),operation,'honcho',IMAGE,password)
        target.launch();target.create_database(settings)
        receipt['repair']=target.restore(archive,source_checks=selected)
        restored=target.restored_metadata()
        inventory.require(restored==manifest['expected'],'repaired_full_catalog_or_content_differs')
        inventory.require(selected_extras(target,target.database,'restored-settings')==settings,'repaired_settings_differs')
        before=json.loads((target.root/'honcho-check-repair-intent.json').read_text())['before']
        inventory.require(before==expected(restored=True),'fresh_restore_did_not_reproduce_actual_cast_delta')
        receipt.update(schema_sha256=restored['schema_sha256'],tables=restored['tables'],
            logical_rows_sha256=restored['logical_rows_sha256'],settings_sha256=sha(settings),
            archive_unchanged=digest(archive)==ARCHIVE_SHA,full_metadata_equal=True,
            schema_comparison_normalized=False,production_offline_restore_exercised=True)
        receipt['owner']=target.stop_retained()
        receipt['status']='passed'
        save(root/'receipt.json',receipt)
    except Exception as error:
        # An unknown failure never starts source services or repairs the retained
        # old target. This exact new owner's identity remains in created-container.json.
        if target is not None and target.container is not None:
            try: receipt['owner']=target.stop_retained()
            except Exception: receipt['owned_fixture_stop_unacknowledged']=True
        receipt.update(status='failed',error_code=str(error) if isinstance(error,Refused) else 'fixture_failed')
        save(root/'receipt.json',receipt)
        raise Refused('honcho_check_fixture_failed_retained',receipt_path=root/'receipt.json') from None
    return root


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-owned-fixture',action='store_true',required=True)
    parser.parse_args()
    root=rehearse()
    print(json.dumps({'status':'passed','receipt':str(root/'receipt.json'),'source_mutations':False}))


if __name__=='__main__':
    try: main()
    except Refused as error:
        print(json.dumps({'status':'failed','receipt':str(error.receipt_path),'source_mutations':False}))
        raise SystemExit(1) from None
