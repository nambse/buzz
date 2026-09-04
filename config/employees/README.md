# Employee manifests

Files in this directory are secret-free, versioned inputs to Ortak employee
provisioning.

- `provisioning: adopt` means Ortak may inspect and bind an existing resource,
  but does not own deletion or destructive compensation.
- `credential_refs` contain opaque `credential://` or `secret://` references.
  They never contain tokens, private keys, passwords, or OAuth payloads.
- Machine-specific overrides use `*.local.yaml`, which git ignores.

`cem.yaml` and `zeynep.yaml` describe the existing deployed test employees.
Their profile paths and public Office identities are safe references; the
profiles themselves remain external to this repository. Both fixtures stay
`status: draft`: importing them cannot make either employee routable. The adopt
saga may create an active revision only after runtime, Honcho, Office membership,
and signer/public-key health checks all succeed.
