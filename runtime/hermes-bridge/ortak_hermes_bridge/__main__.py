"""Private bridge service; wildcard container bind and execution are explicit opt-ins."""
import argparse
import json
import os
import sqlite3
from pathlib import Path
from .journal import BridgeError, Journal
from .service import Bridge, profile_registry, serve


def configured_bridge(config, journal, enable_docker=False):
    """Keep execution unavailable unless the operator explicitly selects a proven image."""
    bridge = Bridge(journal, config['company_id'], config['profiles'])
    if enable_docker:
        from .docker_executor import DockerEngine, DockerExecutor
        settings = config.get('executor', {})
        if set(settings) - {'image', 'network', 'validated_digest', 'workspace_validated_digest', 'confidential_validated_digest', 'docker_binary', 'journal_volume'}:
            raise BridgeError('invalid_executor_configuration')
        try:
            engine = DockerEngine(settings.get('docker_binary', '/usr/bin/docker'))
            executor = DockerExecutor(journal, config['company_id'], bridge.profiles,
                                      settings['image'], settings['network'], engine,
                                      validated_digest=settings['validated_digest'],
                                      workspace_validated_digest=settings.get('workspace_validated_digest'),
                                      confidential_validated_digest=settings.get('confidential_validated_digest'),
                                      journal_volume=settings.get('journal_volume'))
        except KeyError:
            raise BridgeError('executor_validation_required', 503) from None
        bridge.executor = executor
    return bridge


def configured_journal(config, path, enable_docker=False):
    """A selected volume must be owned and mounted before Journal can create or change it."""
    settings = config.get('executor', {})
    if settings.get('journal_volume') is not None:
        if not enable_docker:
            raise BridgeError('executor_validation_required', 503)
        if set(settings) - {'image', 'network', 'validated_digest', 'workspace_validated_digest', 'confidential_validated_digest', 'docker_binary', 'journal_volume'}:
            raise BridgeError('invalid_executor_configuration')
        from .docker_executor import DockerEngine
        from .journal_volume import mount
        mount(DockerEngine(settings.get('docker_binary', '/usr/bin/docker')),
              settings['journal_volume'], config['company_id'], path)
    return Journal(path)


def main():
    """Credentials are mounted files; HTTP defaults to loopback in both modes."""
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--token-file', required=True)
    parser.add_argument('--journal', required=True)
    parser.add_argument('--port', type=int, default=8650)
    parser.add_argument('--listen-address', choices=('127.0.0.1', '0.0.0.0'), default='127.0.0.1')
    parser.add_argument('--enable-validated-docker-executor', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    if sqlite3.sqlite_version_info < (3, 51, 3):
        raise BridgeError('sqlite_wal_fix_required', 503)
    path = Path(args.config)
    if path.stat().st_size > 256 * 1024:
        raise BridgeError('configuration_too_large')
    config = json.loads(path.read_text())
    config['profiles'] = profile_registry(config['profiles'], config['company_id'])
    with open(args.token_file) as token_file:
        token = token_file.read(4097).strip()
    if not 32 <= len(token) <= 4096:
        raise BridgeError('invalid_service_credential')
    journal = configured_journal(config, args.journal, args.enable_validated_docker_executor)
    bridge = configured_bridge(config, journal, args.enable_validated_docker_executor)
    try:
        serve(bridge, token, args.port, args.listen_address)
    finally:
        close = getattr(bridge.executor, 'close', None)
        if close is not None:
            close()

if __name__ == '__main__':
    main()
