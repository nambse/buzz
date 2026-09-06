"""The real OCI export validator must bind manifest IDs, config/layers and their bytes."""

import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

from backup_private_database import Refused
from rehearse_private_recovery_capture_tail import image_export_witness


class TailTests(unittest.TestCase):
    def test_manifest_identity_is_distinct_from_config_and_every_blob_is_hashed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config, layer = b'{"fixture":true}', b'fixture layer bytes'
            descriptor = lambda raw: {'digest': 'sha256:' + hashlib.sha256(raw).hexdigest(), 'size': len(raw)}
            manifest = json.dumps({'schemaVersion': 2, 'config': descriptor(config), 'layers': [descriptor(layer)]}).encode()
            image = descriptor(manifest)['digest']
            index = json.dumps({'schemaVersion': 2, 'manifests': [descriptor(manifest)]}).encode()
            for label, layer_bytes in [('valid', layer), ('corrupt', b'x' * len(layer))]:
                path = root / (label + '.tar')
                with tarfile.open(path, 'w') as archive:
                    for name, raw in [('index.json', index),
                        ('blobs/sha256/' + image[7:], manifest),
                        ('blobs/sha256/' + descriptor(config)['digest'][7:], config),
                        ('blobs/sha256/' + descriptor(layer)['digest'][7:], layer_bytes)]:
                        member = tarfile.TarInfo(name); member.size = len(raw)
                        archive.addfile(member, io.BytesIO(raw))
                if label == 'valid':
                    result = image_export_witness(path, [image])
                    self.assertEqual(result['blob_count'], 3)
                    self.assertEqual(result['identity_kind'], 'oci_manifest_digest')
                    with self.assertRaisesRegex(Refused, 'export_changed'):
                        image_export_witness(path, [descriptor(config)['digest']])
                else:
                    with self.assertRaisesRegex(Refused, 'blob_changed'): image_export_witness(path, [image])


if __name__ == '__main__': unittest.main()
