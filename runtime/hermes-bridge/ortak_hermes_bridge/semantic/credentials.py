"""Selected OAuth read and one owned maintenance lane, without inference payloads."""
import threading
import time

from ..journal import BridgeError
from ..oauth_credentials import token_expiry


def ready_token(store):
    """A score cannot refresh, wait for rotation, or extend its deadline."""
    with store.locked(timeout=0, readonly=True):
        state = store.read()
        if state['phase'] != 'ready' or state['retry_at'] > time.time():
            raise BridgeError('semantic_credential_unavailable', 503)
        token = state['tokens']['access_token']
        if token_expiry(token) <= time.time() + 60:
            raise BridgeError('semantic_credential_unavailable', 503)
        return token


class Maintenance:
    """Enabling the selected service explicitly authorizes this store's refresh only.

    OAuthStore commits its existing uncertainty fence before remote rotation and
    owns the 35-second process deadline plus exact reap. No scoring payload or
    inference result enters this thread. Shutdown waits for this bounded owner.
    """
    def __init__(self, store):
        self.store = store
        self.stopping = threading.Event()
        self.status = 'starting'
        self.thread = threading.Thread(target=self._run, name='ortak-semantic-oauth', daemon=False)

    def start(self):
        self.thread.start()

    def _run(self):
        while not self.stopping.is_set():
            try:
                self.status = 'checking'
                self.store.access_token()
                self.status = 'ready'
            except Exception:
                # Exact recovery/uncertainty/retry-at remains in the owned store.
                self.status = 'unavailable'
            self.stopping.wait(15)
        self.status = 'stopped'

    def close(self):
        self.stopping.set()
        self.thread.join(timeout=40)
        if self.thread.is_alive():
            self.status = 'containment_failed'
            raise BridgeError('credential_maintenance_not_stopped', 503)
