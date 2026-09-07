"""Public, operator-owned delegation to one existing OAuth connection."""
import hashlib
import json

from .journal import BridgeError
from .oauth_credentials import oauth_identity


def connection_identity(company, profile, profiles):
    """Validate an exact registered grant before opening an OAuth directory.

    The owner is an undelegated profile in this same frozen registry. Multiple
    model variants may name that one owner, but no delegation chain is followed.
    Nothing in a RunSpec or an HTTP request can supply this selection.
    """
    if 'oauth_owner' not in profile:
        return oauth_identity(company, profile['employee_id'], profile['binding'])
    try:
        owner = profile['oauth_owner']
        consumer = oauth_identity(company, profile['employee_id'], profile['binding'])
        if (not isinstance(owner, dict)
                or set(owner) != {'format', 'company_id', 'employee_id', 'profile_ref', 'credential_ref'}
                or owner['company_id'] != company
                or owner['credential_ref'] != consumer['credential_ref']
                or owner['employee_id'] == consumer['employee_id']
                or owner != oauth_identity(company, owner['employee_id'], {
                    'profile_ref': owner['profile_ref'], 'credential_refs': [owner['credential_ref']]})
                or not isinstance(profile.get('oauth_directory'), str)
                or not profile['oauth_directory']
                or profile not in profiles):
            raise ValueError()
        matches = [candidate for candidate in profiles
                   if 'oauth_owner' not in candidate
                   and candidate.get('oauth_directory') == profile['oauth_directory']
                   and candidate.get('employee_id') == owner['employee_id']
                   and candidate['binding'].get('profile_ref') == owner['profile_ref']
                   and candidate['binding'].get('credential_refs') == [owner['credential_ref']]]
        if not matches:
            raise ValueError()
        return dict(owner)
    except (KeyError, TypeError, ValueError):
        raise BridgeError('invalid_oauth_connection_grant', 503) from None


def connection_fingerprint(owner, directory):
    """Bind delegated probe evidence to public owner and exact selected store."""
    encoded = json.dumps({'owner': owner, 'directory': directory},
                         sort_keys=True, separators=(',', ':')).encode()
    return hashlib.sha256(b'ortak-oauth-connection/1\0' + encoded).hexdigest()
