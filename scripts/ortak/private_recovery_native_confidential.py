"""Explicit77/78 selection and watched cold capture of the separate native ciphertext store."""
import hashlib
import os
from pathlib import Path

from backup_private_database import private_binary
import private_recovery_inventory as inventory
import recovery_native_confidential as store


def selection(value, schema, native):
    """Historical bundles omit this component;77/78 bind code, path and exact native owner."""
    inventory.require(type(schema) is int and 61<=schema<=78,'native_confidential_selection_refused')
    if schema < 77:
        inventory.require(value is None,'native_confidential_selection_refused')
        return None
    inventory.require(schema in (77,78) and isinstance(value,dict)
        and set(value)=={'app_data','source_sha256','native_owner_sha256'}
        and str(store.absolute(value['app_data']))==value['app_data']
        and value['source_sha256']==hashlib.sha256(Path(store.__file__).read_bytes()).hexdigest()
        and value['native_owner_sha256']==hashlib.sha256(store.canonical(native)).hexdigest(),
        'native_confidential_selection_refused')
    return dict(value)


def prepare(app_data, schema, native):
    """Selecting the fixed store never enumerates or copies an ordinary native profile."""
    value=None if app_data is None else {'app_data':str(app_data),
        'source_sha256':hashlib.sha256(Path(store.__file__).read_bytes()).hexdigest(),
        'native_owner_sha256':hashlib.sha256(store.canonical(native)).hexdigest()}
    return selection(value,schema,native)


def capture(backend):
    """The existing live barrier answers both child observations with fresh stopped-owner evidence."""
    import recovery_native_ingress as ingress
    import private_recovery_workspace_capture as watched
    selected=selection(backend.observation.get('native_confidential'),inventory.MAIN_SCHEMA_VERSION,
        backend.observation['native_ingress'])
    inventory.require(selected is not None,'native_confidential_selection_refused')
    witness=getattr(backend,'held_witness',None)
    def stopped():
        inventory.require(witness is not None and witness.active and witness.process.poll() is None,
            'native_confidential_barrier_required')
        witness.gate.stopped_owners()
        ingress.require_stopped(witness.gate.inspector,backend.observation['native_ingress'])
        return {'owner_sha256':selected['native_owner_sha256']}
    target=backend.output/'native-confidential.tar'
    receipt=watched.bounded_action('native-confidential-capture',
        {'app_data':selected['app_data'],'archive':str(target),
         'expected_owner_sha256':selected['native_owner_sha256']},backend.command,observation=stopped)
    stopped()
    return {'path':target.name,**receipt['archive'],'selection':selected,'receipt':receipt}


def capture_in_child(value, observe):
    """Only the bounded child handles ciphertext bytes; no credential enters its request."""
    inventory.require(set(value)=={'app_data','archive','expected_owner_sha256'},
        'native_confidential_selection_refused')
    path=store.absolute(value['archive'])
    with private_binary(path) as output:
        result=store.write(value['app_data'],output,
            stopped_native=lambda:observe()['owner_sha256'],expected_owner_sha256=value['expected_owner_sha256'])
        output.flush();os.fsync(output.fileno())
    parent=os.open(path.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:os.fsync(parent)
    finally:os.close(parent)
    return result


def extract_in_child(value):
    """Require byte-identical offline readback, preserving rollback journals without replaying them."""
    inventory.require(set(value)=={'app_data','archive','destination','receipt'},
        'native_confidential_selection_refused')
    path=store.absolute(value['archive'])
    descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(descriptor,'rb') as stream:
        result=store.extract(stream,store.absolute(value['destination']),value['receipt'],app_data=value['app_data'])
    inventory.require(result==value['receipt'],'native_confidential_restore_changed')
    return {'status':'ciphertext_preserved_offline','receipt':result,'automatic_activation':False,
        'sqlite_recovery_performed':False,'keys_exported':False,'physical_erasure':False}
