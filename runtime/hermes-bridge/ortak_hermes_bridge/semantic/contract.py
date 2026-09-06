"""Closed, bounded score protocol and explicit immutable profile selection."""
import hashlib
import json
import math
import re
from uuid import UUID

from ..hermes_candidate import runtime_reasoning
from ..journal import BridgeError
from ..oauth_credentials import OAuthStore, oauth_identity
from ..service import profile_registry
from . import EVIDENCE, MAX_BYTES, PROMPT_VERSION, SCHEMA_VERSION


def reject(code='invalid_semantic_request'):
    raise BridgeError(code, 422)


def object_keys(value, expected):
    if not isinstance(value, dict) or set(value) != set(expected):
        reject()


def strict_json(raw):
    """Do not let duplicate JSON control fields or non-finite values disappear."""
    if not isinstance(raw, (bytes, str)) or len(raw if isinstance(raw, bytes) else raw.encode()) > MAX_BYTES:
        reject('semantic_bounds')
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                reject()
            result[key] = value
        return result
    try:
        return json.loads(raw, object_pairs_hook=pairs, parse_constant=lambda _: reject())
    except (ValueError, TypeError, UnicodeError, RecursionError):
        reject()


def canonical_uuid(value):
    try:
        identifier = UUID(value)
        if not identifier.int or str(identifier) != value:
            reject()
    except (ValueError, TypeError, AttributeError):
        reject()
    return value


def text(value, limit=16384):
    if (not isinstance(value, str) or len(value.encode()) > limit
            or any(ord(c) < 32 and c not in '\n\t' for c in value)):
        reject('semantic_bounds')
    return value


def input_candidates(value):
    object_keys(value, ('message', 'candidates'))
    text(value['message'])
    candidates = value['candidates']
    if not isinstance(candidates, list) or not 1 <= len(candidates) <= 32:
        reject('semantic_bounds')
    ids = set()
    for candidate in candidates:
        object_keys(candidate, ('employee_id', 'name', 'title', 'biography', 'responsibilities', 'domains'))
        identifier = text(candidate['employee_id'], 128)
        if not identifier or identifier in ids:
            reject()
        ids.add(identifier)
        for field in ('name', 'title', 'biography'):
            text(candidate[field])
        for field in ('responsibilities', 'domains'):
            if not isinstance(candidate[field], list) or len(candidate[field]) > 32:
                reject('semantic_bounds')
            for entry in candidate[field]:
                text(entry)
    if len(json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()) > MAX_BYTES:
        reject('semantic_bounds')
    return ids


def scores(value, expected):
    object_keys(value, ('scores',))
    items = value['scores']
    if not isinstance(items, list) or len(items) != len(expected):
        reject('invalid_semantic_scores')
    seen = set()
    for item in items:
        object_keys(item, ('employee_id', 'score', 'evidence'))
        identifier = item['employee_id']
        if (not isinstance(identifier, str) or identifier not in expected or identifier in seen
                or type(item['score']) not in (int, float) or not math.isfinite(item['score'])
                or not 0 <= item['score'] <= 1 or not isinstance(item['evidence'], str)
                or item['evidence'] not in EVIDENCE):
            reject('invalid_semantic_scores')
        seen.add(identifier)
    return sorted(items, key=lambda item: item['employee_id'])


class Selection:
    """A single central deployment explicitly using one existing owned OAuth identity."""
    def __init__(self, config):
        object_keys(config, ('company_id', 'profiles', 'semantic'))
        self.company_id = canonical_uuid(config['company_id'])
        selected = config['semantic']
        object_keys(selected, ('deployment_id', 'binding_sha256', 'response_model'))
        self.deployment_id = canonical_uuid(selected['deployment_id'])
        self.binding_sha256 = selected['binding_sha256']
        if not isinstance(self.binding_sha256, str) or not re.fullmatch('[0-9a-f]{64}', self.binding_sha256):
            reject('invalid_semantic_selection')
        self.response_model = selected['response_model']
        if not isinstance(self.response_model, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}', self.response_model):
            reject('invalid_semantic_selection')
        profiles = profile_registry(config['profiles'])
        matches = [p for p in profiles if hashlib.sha256(json.dumps(p['binding'],
            sort_keys=True, separators=(',', ':')).encode()).hexdigest() == self.binding_sha256]
        if len(matches) != 1 or 'oauth_directory' not in matches[0]:
            reject('semantic_profile_not_found')
        profile = matches[0]
        self.binding = profile['binding']
        self.effort = runtime_reasoning(self.binding, 'openai-codex')
        self.model = self.binding['model']
        # All public configuration gates precede opening the explicitly owned store.
        self.store = OAuthStore(profile['oauth_directory'],
            oauth_identity(self.company_id, profile['employee_id'], self.binding))

    def request(self, body):
        object_keys(body, ('deployment_id', 'binding_sha256', 'request_id',
                           'prompt_version', 'schema_version', 'budget_ms', 'input'))
        if (body['deployment_id'] != self.deployment_id or body['binding_sha256'] != self.binding_sha256
                or body['prompt_version'] != PROMPT_VERSION or body['schema_version'] != SCHEMA_VERSION
                or type(body['budget_ms']) is not int or not 1 <= body['budget_ms'] <= 4500):
            reject('semantic_selection_changed')
        canonical_uuid(body['request_id'])
        return input_candidates(body['input'])

    def response(self, result, usage):
        return {'deployment_id': self.deployment_id, 'binding_sha256': self.binding_sha256,
            'model': self.response_model, 'reasoning_effort': self.effort,
            'prompt_version': PROMPT_VERSION, 'schema_version': SCHEMA_VERSION,
            'scores': result, 'usage': usage}
