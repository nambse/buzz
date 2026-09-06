"""Destination inheritance at the real restore admission and exclusive-move seams.

Only synthetic public metadata/opaque bytes and fresh temporary directories are
used. No database, Docker service, recovery key or selected native store opens.
"""
from contextlib import ExitStack
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from backup_private_database import Refused
import restore_private_recovery as subject


class DestinationTests(unittest.TestCase):
    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory(prefix='ortak-destination-unit-')
        self.addCleanup(self.temporary.cleanup)
        self.root=Path(self.temporary.name).resolve()
        self.root.chmod(0o700)

    def native_row(self, bundle, selected, group, *, absent=False, file_group=None):
        native=subject.native_ciphertext.store
        body=b'opaque ciphertext fixture; never decoded'
        root=None if absent else dict(mode=0o700,uid=os.getuid(),gid=group,mtime_ns=0)
        files=[] if absent else [dict(name=native.DATABASE,mode=0o600,uid=os.getuid(),
            gid=group if file_group is None else file_group,mtime_ns=0,bytes=len(body),
            sha256=hashlib.sha256(body).hexdigest())]
        store=native.metadata(native.absolute(selected['app_data']),root,files)
        stream=io.BytesIO();raw=native.canonical(store)
        native.wire.header(stream,native.MANIFEST,
            dict(mode=0o600,uid=os.getuid(),gid=os.getgid(),mtime_ns=0),len(raw))
        stream.write(raw+bytes(-len(raw)%512))
        if not absent:
            native.wire.header(stream,'.',root,0,root=True)
            native.wire.header(stream,native.DATABASE,files[0],len(body))
            stream.write(body+bytes(-len(body)%512))
        stream.write(bytes(1024));raw=stream.getvalue()
        path=bundle/'native-confidential.tar';path.write_bytes(raw);path.chmod(0o600)
        archive=dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
        return dict(path=path.name,**archive,selection=selected,
            receipt=dict(store=store,archive=archive,stopped_native_sha256=selected['native_owner_sha256']))

    def test_native_group_admission_precedes_decrypt_and_store_creation(self):
        for name in ('wrong-root','wrong-file','matching','absent'):
            with self.subTest(case=name), ExitStack() as stack:
                base=self.root/name;base.mkdir(mode=0o700)
                bundle=base/'bundle';bundle.mkdir(mode=0o700)
                group=base.stat().st_gid
                owner={'native':'synthetic stopped owner'}
                selected=subject.native_ciphertext.prepare(base/'app',78,owner)
                row=self.native_row(bundle,selected,group+1 if name=='wrong-root' else group,
                    absent=name=='absent',file_group=group+1 if name=='wrong-file' else group)
                contract={'main':{'schema_version':78}}
                prepared={'observation':{'files':{'secret_metadata_only':[]},
                    'containers':{'controller':{}},'native_ingress':owner,'native_confidential':selected},
                    'plan':{'recovery_contract':contract}}
                manifest={'manifest_sha256':'ab'*32,'components':{'journal':{},
                    'native_confidential':row,'public_artifacts':{'configuration':{}},
                    'databases':{'main':{'recovery_obligations':{'evidence':{}}},'honcho':{}}}}
                for kind in ('main','honcho'):
                    (bundle/(kind+'-database.json')).write_text('{"expected":{}}')
                config=bundle/'configuration.tar';config.write_bytes(b'public fixture')
                real_artifact=subject.artifact
                def artifact(parent, item, expected, limit):
                    return config if expected=='configuration.tar' else real_artifact(parent,item,expected,limit)
                def configuration(stream, limit, output):
                    (output/'operation').mkdir(mode=0o700)
                    (output/'operation/preparation.json').write_bytes(subject.canonical(prepared))
                    return {}
                stack.enter_context(patch.object(subject.inventory,'STATE',base))
                stack.enter_context(patch.object(subject,'uuid4',return_value=SimpleNamespace(hex='cd'*16)))
                stack.enter_context(patch.object(subject,'load_bundle',return_value=manifest))
                stack.enter_context(patch.object(subject,'preflight',return_value={}))
                stack.enter_context(patch.object(subject,'artifact',side_effect=artifact))
                stack.enter_context(patch.object(subject.shutil,'disk_usage',return_value=SimpleNamespace(free=8*1024**3)))
                stack.enter_context(patch.object(subject.recovery_archive_io,'archive',side_effect=configuration))
                stack.enter_context(patch.object(subject.scorer,'verify_offline',return_value=None))
                stack.enter_context(patch.object(subject.obligations,'stack_contract',return_value=contract))
                stack.enter_context(patch.object(subject.selected_journal,'require_confidential_schema',return_value=None))
                stack.enter_context(patch.object(subject.obligations.workspaces,'require_capture_selection'))
                stack.enter_context(patch.object(subject.obligations.workspaces,'require_capture_scope'))
                decrypt=stack.enter_context(patch.object(subject,'decrypt',side_effect=Refused('fixture_decrypt_boundary')))
                postgres=stack.enter_context(patch.object(subject.stores,'Postgres'))
                volumes=stack.enter_context(patch.object(subject.stores,'restore_volume'))
                with self.assertRaisesRegex(Refused,'offline_restore_failed_retained'):
                    subject.restore(bundle/'manifest.json')
                output=base/'recovery-offline-restores'/('cd'*16)
                result=json.loads((output/'manifest.json').read_bytes())
                proof=json.loads((output/'native-confidential-destination.json').read_bytes())
                compatible=name in ('matching','absent')
                self.assertIs(proof['compatible'],compatible)
                self.assertEqual(result['failure_code'],'fixture_decrypt_boundary' if compatible
                    else 'offline_native_destination_group_mismatch')
                self.assertEqual(decrypt.call_count,int(compatible))
                self.assertEqual((output/'secret-material').exists(),compatible)
                self.assertFalse(proof['ownership_changed']);self.assertFalse(proof['ciphertext_extracted'])
                self.assertFalse((output/'native-confidential').exists())
                postgres.assert_not_called();volumes.assert_not_called()

    @unittest.skipUnless(sys.platform=='darwin','Darwin exclusive descriptor rename contract')
    def test_workspace_staging_move_and_collision_keep_original_inodes(self):
        for collision in (False,True):
            with self.subTest(collision=collision), patch.object(subject.inventory,'STATE',self.root):
                output=self.root/('collision' if collision else 'move');output.mkdir(mode=0o700)
                target=output/'workspace-files'
                if collision:target.mkdir(mode=0o700)
                prior=target.stat().st_ino if collision else None
                plan={'compatible':True,'output':str(output),'parent':str(self.root),
                    'strategy':'inherited_group_move','manifest_sha256':'ef'*32,
                    'output_identity':subject.directory_identity(output.stat()),
                    'parent_identity':subject.directory_identity(self.root.stat())}
                if collision:
                    with self.assertRaises(FileExistsError):subject.workspace_destination(output,plan)
                else:
                    self.assertEqual(subject.workspace_destination(output,plan),target)
                staged=json.loads((output/'workspace-destination-staged.json').read_bytes())
                source=self.root/staged['staged_name']
                if collision:
                    self.assertEqual(target.stat().st_ino,prior)
                    self.assertEqual(source.stat().st_ino,staged['identity']['inode'])
                    self.assertEqual(list(target.iterdir()),[]);self.assertEqual(list(source.iterdir()),[])
                    self.assertTrue((output/'workspace-destination-failure.json').exists())
                    self.assertFalse((output/'workspace-destination-created.json').exists())
                else:
                    self.assertFalse(source.exists())
                    self.assertEqual(subject.directory_identity(target.stat()),staged['identity'])
                    self.assertTrue((output/'workspace-destination-created.json').exists())


if __name__=='__main__':unittest.main()
