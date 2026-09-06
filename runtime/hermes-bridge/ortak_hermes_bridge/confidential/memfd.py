"""Linux-only sealed volatile stdin. Unsupported hosts never fall back to disk."""
import fcntl
import os
import subprocess
from ..journal import BridgeError

MAX_CHILD_BODY = 128 * 1024


def supported():
    return (hasattr(os, 'memfd_create') and hasattr(os, 'MFD_ALLOW_SEALING')
            and all(hasattr(fcntl, name) for name in ('F_ADD_SEALS', 'F_GET_SEALS', 'F_SEAL_WRITE',
                    'F_SEAL_GROW', 'F_SEAL_SHRINK', 'F_SEAL_SEAL')))


def launch(binary, args, payload):
    """A bounded, sealed, CLOEXEC descriptor is duped to the child's stdin only."""
    if not supported(): raise BridgeError('confidential_memfd_unavailable', 503)
    if type(payload) is not bytes or not 0 < len(payload) <= MAX_CHILD_BODY:
        raise BridgeError('body_too_large', 413)
    descriptor = None
    try:
        descriptor = os.memfd_create('ortak-confidential-stdin', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
        offset = 0
        while offset < len(payload):
            written = os.write(descriptor, payload[offset:])
            if written <= 0: raise OSError()
            offset += written
        os.lseek(descriptor, 0, os.SEEK_SET)
        seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(descriptor, fcntl.F_ADD_SEALS, seals)
        if fcntl.fcntl(descriptor, fcntl.F_GET_SEALS) & seals != seals: raise OSError()
        return subprocess.Popen([binary, *args], stdin=descriptor, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, close_fds=True)
    except OSError:
        pass
    finally:
        if descriptor is not None: os.close(descriptor)
    raise BridgeError('confidential_launch_failed', 503)
