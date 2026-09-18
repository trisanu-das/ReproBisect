# Security and privacy

## Security model

ReproBisect executes the configured project build command. Build input must therefore be treated as **untrusted code execution inside the selected OCI runtime**, not as passive file analysis.

Docker and Podman provide process/filesystem isolation for experiments, but ReproBisect is **not a hardened sandbox for hostile code**. In particular, a Docker daemon or privileged container runtime is itself a high-value host capability. ReproBisect never intentionally mounts the host Docker socket into the build container and does not automatically inject host credentials.

Use a dedicated CI worker or otherwise disposable build host for projects you do not trust.

## Secrets

Do not put long-lived credentials in `[build.env]` unless the build genuinely requires them. ReproBisect redacts configured environment-variable values in persisted environment evidence, but it cannot prevent a build command from printing a secret to stdout/stderr. Captured build text is bounded, not semantically redacted.

Network/syscall traces are processed into coarse summaries; raw syscall text and exact remote endpoints are temporary. Custom package-cache paths and cache member names are not persisted in normal evidence. Source/dependency hashes and selected standardized package metadata are persisted because they are diagnostic evidence.

## Fix safety

`reprobisect fix --verify` applies generated project patches only to a temporary source copy. Existing-file patches carry SHA-256 stale-file preconditions. ReproBisect does not silently edit the user's checkout.

## Reporting vulnerabilities

When ReproBisect is hosted in a public repository, report security vulnerabilities through that repository's private security-advisory channel where available. Do not include credentials, private source, raw proprietary build logs, or other secrets in a public issue.

## Supported security line

The `1.0.0` line remains the current published/supported stable line until `v1.1.0` is released. The `develop/1.1.0` branch carries the final 1.1.0 source version under release qualification; it is not a separately supported stable security line until publication. Earlier `1.0.0-rc.*` and `0.1.0-alpha.*` snapshots are historical development artifacts.
