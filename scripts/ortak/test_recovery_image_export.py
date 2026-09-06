"""Production command/capture/preflight seams with real bounded stdlib children; no Docker."""
from contextlib import contextmanager
import copy
import gzip
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import backup_private_database as backup
import capture_private_recovery as capture
import recovery_image_export as images
import restore_private_recovery as restore

IMAGE='sha256:'+'a'*64
DATA=b'bounded image archive fixture\n'*50000
PROGRAM="import sys; sys.stdout.buffer.write(b'bounded image archive fixture\\n'*50000)"


class ImageCommands(backup.Commands):
    """Substitute only the Docker invocation; production process/stream/cleanup code stays real."""
    program=PROGRAM
    def docker(self,*args):
        if args==('image','inspect','--format','{{.Id}}',IMAGE):
            return [sys.executable,'-c',"print('"+IMAGE+"')"]
        if args==('image','save',IMAGE):return [sys.executable,'-c',self.program]
        raise AssertionError('unexpected Docker request')


class ImageExportTests(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory();self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name).resolve();self.root.chmod(0o700)

    def backend(self,root):
        value=capture.Capture.__new__(capture.Capture);value.output=root
        value.prepared={'plan':{'images':[IMAGE]}};value.command=ImageCommands(root)
        value.gzip_images=True;value.image_output_limit=128*1024
        return value

    def test_real_capture_stream_footer_fsync_and_closed_offline_preflight(self):
        backend=self.backend(self.root);fsync=os.fsync;finished=[]
        def completed(descriptor):
            # The real footer is readable before the durability boundary, not appended by close later.
            finished.append(gzip.decompress((self.root/'images.tar.gz').read_bytes()))
            fsync(descriptor)
        with patch.object(backup.os,'fsync',side_effect=completed):row=backend.images()
        self.assertEqual(finished,[DATA]);self.assertFalse((self.root/'images.tar').exists())
        raw=(self.root/'images.tar.gz').read_bytes()
        self.assertEqual(raw[:10],images.GZIP_HEADER)
        self.assertEqual(row['bytes'],len(raw));self.assertLess(len(raw),len(DATA))
        self.assertEqual(row['uncompressed_bytes'],len(DATA))
        self.assertEqual(row['uncompressed_sha256'],hashlib.sha256(DATA).hexdigest())
        self.assertTrue(images.verify_gzip(self.root/row['path'],row,8*1024**3)['footer_verified'])
        manifest=self.manifest(row)
        with patch.object(restore,'decrypt',side_effect=AssertionError('preflight must not read keys')):
            self.assertIn(self.root/'images.tar.gz',restore.preflight(self.root,manifest))
            for key,value in [('path','images.tar'),('compression','none'),('format','unknown/2'),
                              ('uncompressed_bytes',len(DATA)-1),('uncompressed_sha256','f'*64)]:
                changed=copy.deepcopy(manifest);changed['components']['images'][key]=value
                with self.subTest(key=key),self.assertRaises(backup.Refused):restore.preflight(self.root,changed)
        # A newly hashed partial archive must still fail footer validation.
        (self.root/'images.tar.gz').write_bytes(raw[:-1])
        truncated=copy.deepcopy(manifest);truncated['components']['images'].update(bytes=len(raw)-1,
            sha256=hashlib.sha256(raw[:-1]).hexdigest())
        with self.assertRaises(backup.Refused):restore.preflight(self.root,truncated)
        (self.root/'images.tar').write_bytes(DATA);(self.root/'images.tar').chmod(0o600)
        legacy={'path':'images.tar','bytes':len(DATA),'sha256':hashlib.sha256(DATA).hexdigest(),'images':[IMAGE]}
        manifest['components']['images']=legacy
        self.assertIn(self.root/'images.tar',restore.preflight(self.root,manifest))
        self.assertEqual(images.selection(legacy,8*1024**3)['compression'],'none')
        with self.assertRaises(backup.Refused):images.options(False,4096,8*1024**3)

    def manifest(self,row):
        def artifact(name):
            path=self.root/name;path.write_bytes(b'bounded other fixture');path.chmod(0o600)
            return {'path':name,'bytes':path.stat().st_size,'sha256':backup.digest(path)}
        databases={kind:{**artifact(kind+'.dump'),'receipt':artifact(kind+'-database.json')} for kind in ('main','honcho')}
        return {'secrets':artifact('secrets.aesgcm'),'components':{'databases':databases,
            'volumes':{kind:artifact(kind+'.tar') for kind in ('redis','minio')},'journal':artifact('journal.sqlite'),
            'public_artifacts':{'configuration':artifact('configuration.tar'),
                'native_and_repositories':artifact('native-and-repositories.tar')},'images':row}}

    def test_real_child_limits_stderr_exit_and_deadline_never_finish_gzip(self):
        cases=[('input',PROGRAM,1024,128*1024,'output_limit'),
            ('physical',"import sys;sys.stdout.buffer.write(bytes(range(256))*4096)",2*1024**2,64,'output_limit'),
            ('stderr',PROGRAM+";sys.stderr.write('synthetic diagnostic')",2*1024**2,128*1024,'reported_diagnostics'),
            ('exit',PROGRAM+';sys.exit(7)',2*1024**2,128*1024,'command_failed'),
            ('deadline',"import time;time.sleep(60)",2*1024**2,128*1024,'deadline')]
        for label,program,ceiling,physical,code in cases:
            command=backup.Commands(self.root);target=self.root/(label+'.tar.gz')
            if label=='deadline':command.deadline=time.monotonic()+0.15
            with self.subTest(label=label),patch.object(backup.Commands,'stop',wraps=backup.Commands.stop) as stop:
                with self.assertRaisesRegex(backup.Refused,code):
                    command.run(label,[sys.executable,'-c',program],output=target,ceiling=ceiling,
                        gzip_output=True,output_ceiling=physical)
                stop.assert_called_once()
                self.assertIsNotNone(stop.call_args.args[0].poll())
            self.assertTrue(target.exists());self.assertLessEqual(target.stat().st_size,physical)
            with self.assertRaises((EOFError,OSError)):gzip.decompress(target.read_bytes())
        occupied=self.root/'occupied.tar.gz';occupied.write_bytes(b'original');occupied.chmod(0o600)
        with self.assertRaises(FileExistsError):
            backup.Commands(self.root).run('occupied',[sys.executable,'-c',PROGRAM],output=occupied,
                ceiling=2*1024**2,gzip_output=True,output_ceiling=128*1024)
        self.assertEqual(occupied.read_bytes(),b'original')

    def test_capture_failure_keeps_partial_archive_receipt_and_explicit_capacity(self):
        root=self.root
        class Backend(capture.Capture):
            def __init__(self,output,registry):
                self.output=output;self.observation={};self.prepared={'plan':{'images':[IMAGE]}}
                self.command=ImageCommands(output)
                self.command.program=PROGRAM+';sys.exit(9)'
            def cold_stores(self):pass
            def databases(self):return {'main':{'recovery_obligations':{'evidence':{}}}}
            def volumes(self):return {}
            def journal(self):return {}
            def public_artifacts(self):return {}
            def secrets(self,components):raise AssertionError('failed images must precede secret capture')
            def current(self):pass
        @contextmanager
        def barrier(*args,**kwargs):yield {'databases':{'recovery_obligations':{}}}
        baseline=sum(value for key,value in capture.CAPTURE_LIMITS.items() if key.endswith('_bytes'))
        physical=128*1024
        reduced=baseline-capture.CAPTURE_LIMITS['image_exports_bytes']+physical
        with patch.object(capture.inventory,'STATE',root),patch.object(capture,'load_registry',return_value={'registry_sha256':'b'*64}), \
                patch.object(capture,'root_pause_receipt',return_value={}), \
                patch.object(capture.shutil,'disk_usage',return_value=SimpleNamespace(free=reduced)):
            with self.assertRaises(backup.Refused):
                capture.capture(root/'owners',root/'pause',backend_type=Backend,barrier=barrier,
                    gzip_images=True,image_output_limit=physical)
        output=next((root/'recovery-bundles').iterdir())
        receipt=json.loads((output/'failure.json').read_bytes())
        self.assertEqual(receipt['failed_phase'],'images');self.assertEqual(receipt['cause_code'],'command_failed')
        self.assertEqual(receipt['image_export_options']['output_limit'],physical)
        self.assertTrue((output/'images.tar.gz').exists());self.assertFalse((output/'images.tar').exists())
        self.assertEqual(json.loads((output/'manifest.json').read_bytes())['status'],'failed')


if __name__=='__main__':unittest.main()
