"""Actual watched-child capture and offline extraction; no native app, keys or Docker."""
import hashlib
from pathlib import Path
import tempfile
import unittest

from backup_private_database import Commands
import private_recovery_native_confidential as subject
import private_recovery_workspace_capture as watched


class NativeCompositionTests(unittest.TestCase):
    def test_explicit77_selection_and_watched_two_observation_roundtrip(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory).resolve();root.chmod(0o700)
            app=root/'app';app.mkdir(mode=0o700)
            store=app/'ortak-encrypted-dm-v1';store.mkdir(mode=0o700)
            source=store/'ciphertext.sqlite';source.write_bytes(bytes(range(256))*4);source.chmod(0o600)
            native={'artifact':{'binary_sha256':'a'*64},'process':None,'running':False}
            selected=subject.prepare(app,77,native)
            self.assertEqual(subject.selection(selected,77,native),selected)
            self.assertEqual(subject.selection(selected,78,native),selected)
            for value,schema in [(None,77),(None,78),(selected,76),(selected,79),(selected,78.0),(selected,True),({**selected,'native_owner_sha256':'b'*64},77)]:
                with self.assertRaises(subject.inventory.Refused):subject.selection(value,schema,native)
            seen=[]
            def observe():
                seen.append(True)
                return {'owner_sha256':selected['native_owner_sha256']}
            commands=root/'commands';commands.mkdir(mode=0o700)
            archive=root/'native-confidential.tar'
            receipt=watched.bounded_action('native-confidential-capture',
                {'app_data':str(app),'archive':str(archive),
                 'expected_owner_sha256':selected['native_owner_sha256']},Commands(commands),observation=observe)
            self.assertEqual(len(seen),2)
            self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(),receipt['archive']['sha256'])
            target=root/'restored'
            result=watched.bounded_action('native-confidential-extract',
                {'app_data':str(app),'archive':str(archive),'destination':str(target),'receipt':receipt},Commands(commands))
            self.assertEqual(result['receipt'],receipt)
            self.assertFalse(result['automatic_activation'])
            self.assertEqual((target/'ciphertext.sqlite').read_bytes(),source.read_bytes())
            bad=root/'refused.tar'
            with self.assertRaises(watched.Refused):
                watched.bounded_action('native-confidential-capture',
                    {'app_data':str(app),'archive':str(bad),'expected_owner_sha256':selected['native_owner_sha256']},
                    Commands(commands),observation=lambda:{'owner_sha256':'b'*64})
            self.assertTrue(bad.exists())
            self.assertEqual(source.read_bytes(),bytes(range(256))*4)


if __name__=='__main__':unittest.main()
