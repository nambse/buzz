"""Private bridge service; wildcard container bind and execution are explicit opt-ins."""
import argparse
import json
import os
import sqlite3
from pathlib import Path
from .journal import BridgeError, Journal
from .service import Bridge, serve


def configured_bridge(config, journal, enable_docker=False):
    """Keep execution unavailable unless the operator explicitly selects a proven image."""
    bridge = Bridge(journal, config['company_id'], config['profiles'])
    if enable_docker:
        from .docker_executor import DockerEngine, DockerExecutor
        settings = config.get('executor', {})
        if set(settings) - {'image', 'network', 'validated_digest', 'docker_binary'}:
            raise BridgeError('invalid_executor_configuration')
        try:
            engine = DockerEngine(settings.get('docker_binary', '/usr/bin/docker'))
            executor = DockerExecutor(journal, config['company_id'], config['profiles'],
                                      settings['image'], settings['network'], engine,
                                      validated_digest=settings['validated_digest'])
        except KeyError:
            raise BridgeError('executor_validation_required', 503) from None
        bridge.executor = executor
    return bridge


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
    with open(args.token_file) as token_file:
        token = token_file.read(4097).strip()
    if not 32 <= len(token) <= 4096:
        raise BridgeError('invalid_service_credential')
    bridge = configured_bridge(config, Journal(args.journal), args.enable_validated_docker_executor)
    try:
        serve(bridge, token, args.port, args.listen_address)
    finally:
        close = getattr(bridge.executor, 'close', None)
        if close is not None:
            close()

if __name__ == '__main__':
    main()
