"""Explicit local Docker journal storage; no create, copy or fallback behavior."""
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import socket
from uuid import UUID

from .journal import BridgeError


def require(ok):
    """Keep rejected storage identities and daemon responses out of errors."""
    if not ok:
        raise BridgeError('journal_volume_ownership_required', 503)


def selection(value):
    """Copy only an exact named-volume selection; None preserves legacy binding."""
    if value is None:
        return None
    try:
        require(isinstance(value, dict) and set(value) == {'name', 'created_at', 'owner_id'})
        require(isinstance(value['name'], str) and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,127}', value['name']))
        require(isinstance(value['owner_id'], str) and str(UUID(value['owner_id'])) == value['owner_id']
                and UUID(value['owner_id']).int != 0)
        stamp = value['created_at']
        require(isinstance(stamp, str) and 20 <= len(stamp) <= 40
                and re.fullmatch(r'[0-9T:.+Z-]+', stamp))
        require(datetime.fromisoformat(stamp).utcoffset() == timezone.utc.utcoffset(None))
    except (TypeError, ValueError, KeyError, AttributeError):
        raise BridgeError('journal_volume_ownership_required', 503) from None
    return dict(value)


def document(engine, args):
    """Use the existing five-second/1024-byte Docker command bound unchanged."""
    code, raw = engine.command(args)
    require(code == 0)
    try:
        value = json.loads(raw)
    except (TypeError, ValueError):
        raise BridgeError('journal_volume_ownership_required', 503) from None
    require(isinstance(value, dict))
    return value


def controller_format(directory):
    """Project only this journal mount and any nested shadows, never full Config."""
    target, prefix = json.dumps(directory), json.dumps(directory + '/')
    length = len(directory.encode())
    relevant = '(or (eq .Destination ' + target + ') (and (gt (len .Destination) ' + str(length) + ') '
    relevant += '(eq (slice .Destination 0 ' + str(length + 1) + ') ' + prefix + ')))'
    return ('{"id":{{json .Id}},"hostname":{{json .Config.Hostname}},'
        '"running":{{json .State.Running}},"pid":{{json .State.Pid}},'
        '"company":{{json (index .Config.Labels "org.ortak.company")}},'
        '"owner":{{json (index .Config.Labels "org.ortak.journal_owner")}},"mounts":['
        '{{$first := true}}{{range .Mounts}}{{if ' + relevant + '}}{{if not $first}},{{end}}{{$first = false}}'
        '{"type":{{json .Type}},"name":{{json .Name}},"source":{{json .Source}},'
        '"destination":{{json .Destination}},"rw":{{json .RW}}}{{end}}{{end}}]}')


def mount(engine, configured, company, journal_path):
    """Verify volume generation and the calling controller's actual mount before use."""
    selected = selection(configured)
    require(selected is not None)
    try:
        require(isinstance(company, str) and str(UUID(company)) == company and UUID(company).int != 0)
    except (TypeError, ValueError):
        raise BridgeError('journal_volume_ownership_required', 503) from None
    path = Path(journal_path)
    require(path.is_absolute() and path.resolve() == path and re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', path.name))
    directory = str(path.parent)
    require(len(directory.encode()) <= 512 and directory.isascii() and ',' not in directory
            and all(ord(c) >= 32 for c in directory))
    hostname = socket.gethostname()
    # Docker's default UTS hostname is its immutable ID prefix. No ambient
    # environment variable or user-selected alias identifies the controller.
    require(re.fullmatch(r'[0-9a-f]{12}', hostname))
    volume = document(engine, ['volume', 'inspect', '--format',
        '{"name":{{json .Name}},"created_at":{{json .CreatedAt}},"driver":{{json .Driver}},'
        '"scope":{{json .Scope}},"options":{{json .Options}},"source":{{json .Mountpoint}},'
        '"company":{{json (index .Labels "org.ortak.company")}},'
        '"owner":{{json (index .Labels "org.ortak.journal_owner")}}}', selected['name']])
    require(volume.get('name') == selected['name'] and volume.get('created_at') == selected['created_at']
            and volume.get('driver') == 'local' and volume.get('scope') == 'local'
            and volume.get('options') in (None, {}) and volume.get('company') == company
            and volume.get('owner') == selected['owner_id'])
    source = volume.get('source')
    require(isinstance(source, str) and source.startswith('/') and len(source) <= 512)
    controller = document(engine, ['container', 'inspect', '--format', controller_format(directory), hostname])
    container_id = controller.get('id')
    require(isinstance(container_id, str) and re.fullmatch(r'[0-9a-f]{64}', container_id)
            and container_id[:12] == hostname and controller.get('hostname') == hostname
            and controller.get('running') is True and type(controller.get('pid')) is int
            and controller['pid'] > 0 and controller.get('company') == company
            and controller.get('owner') == selected['owner_id'])
    require(controller.get('mounts') == [{'type': 'volume', 'name': selected['name'],
            'source': source, 'destination': directory, 'rw': True}])
    # An attached volume cannot be removed/recreated between this check and
    # launch. nocopy prevents image contents initializing or changing its bytes.
    return f"type=volume,src={selected['name']},dst=/ortak-state,volume-nocopy"
