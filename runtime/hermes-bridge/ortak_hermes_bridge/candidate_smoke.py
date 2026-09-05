"""Image-only guard smoke against real pinned AIAgent; no model/network calls."""
import json
import os
import sqlite3
import sys
import tempfile
from pathlib import Path
from uuid import uuid4
from . import HERMES_REVISION
from .journal import Journal
from .hermes_candidate import ToolDenied, guarded_agent_class, agent_constructor_kwargs
from .verify_source import verify_source
from .worker import prepare_home

BOUNDARIES = ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
              '_execute_tool_calls_concurrent', '_dispatch_delegate_task')

class ForbiddenSmokeIO(BaseException):
    """Network/process access fails this isolated smoke even if upstream catches Exception."""


def main():
    """Validate real initialization and each production tool entry without credentials."""
    if sqlite3.sqlite_version_info < (3, 51, 3):
        raise RuntimeError('SQLite WAL-reset fix is required')
    lock = verify_source()
    attempted = []
    def audit(event, args):
        if event in {'socket.connect', 'socket.getaddrinfo', 'subprocess.Popen',
                     'os.system', 'os.exec', 'os.posix_spawn', 'os.fork'}:
            attempted.append(event)
            raise ForbiddenSmokeIO()
    sys.addaudithook(audit)
    with tempfile.TemporaryDirectory(prefix='ortak-guard-') as temporary:
        prepare_home(Path(temporary) / 'hermes-home')
        sys.path.insert(0, '/opt/hermes')
        from run_agent import AIAgent
        from model_tools import get_tool_definitions
        if get_tool_definitions(enabled_toolsets=[], disabled_toolsets=[], quiet_mode=True):
            raise RuntimeError('empty tool selection expanded')
        key = f'ortak-run:{uuid4()}:{uuid4()}'
        run_id = key.split(':')[2]
        journal = Journal(Path(temporary) / 'journal.sqlite')
        journal.reserve({'idempotency_key': key, 'run_id': run_id})
        if not journal.begin_execution(key):
            raise RuntimeError('smoke execution admission failed')
        guarded = guarded_agent_class(AIAgent, journal, key)
        constructor_kwargs = agent_constructor_kwargs(
            {'run_id': run_id, 'binding': {'model': 'gpt-4o-mini'}},
            'openai', 'fixture-only-not-a-provider-key')
        agent = guarded(**constructor_kwargs)
        if agent.api_key != constructor_kwargs['api_key'] or agent.base_url != constructor_kwargs['base_url']:
            raise RuntimeError('real constructor did not retain the selected credential route')
        if agent._environment_probe:
            raise RuntimeError('real constructor enabled environment probing')
        if agent.tools or agent.valid_tool_names:
            raise RuntimeError('real constructor enabled tools')
        for boundary in BOUNDARIES:
            try:
                getattr(agent, boundary)('terminal', {'command': 'unreachable fixture command'})
            except ToolDenied:
                continue
            raise RuntimeError('tool boundary did not deny')
        events = journal.events(key)
        if journal.lookup(key)['status'] != 'failed' or not events['terminal']:
            raise RuntimeError('tool denial did not persist')
        if attempted:
            raise RuntimeError('real constructor attempted network/process access')
        print(json.dumps({'source_revision': HERMES_REVISION,
                          'verified_source_files': len(lock['source_files']),
                          'sqlite_version': sqlite3.sqlite_version,
                          'real_agent_constructor': 'passed', 'denied_boundaries': list(BOUNDARIES),
                          'model_calls': 0, 'network_calls': 0,
                          'durable_policy_denial': True}, sort_keys=True))

if __name__ == '__main__':
    main()
