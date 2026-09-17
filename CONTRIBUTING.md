# Contributing to Soroban Forge

Thank you for your interest in contributing to Soroban Forge! This document provides guidelines and steps for contributing.

## Code of Conduct

By participating, you agree to uphold our Code of Conduct. Be respectful, inclusive, and collaborative.

## How to Contribute

### Reporting Bugs

- Use the **bug_report** issue template.
- Include steps to reproduce, expected behavior, actual behavior, and environment details (OS, Rust version, Soroban SDK version).
- Attach logs or minimal reproduction repositories when possible.

### Feature Requests

- Use the **feature_request** issue template.
- Describe the use case, proposed solution, and alternatives considered.
- Label the issue with `enhancement` and `good first issue` if appropriate.

### Security Issues

- **Do NOT open a public issue** for security vulnerabilities.
- Report confidentially via the repository's Security tab (GitHub private
  vulnerability reporting / Security Advisories).
- See [SECURITY.md](SECURITY.md) for details.

## Development Setup

The workspace tracks **stable Rust** via `rust-toolchain.toml` and builds
contracts for the `wasm32v1-none` target required by soroban-sdk 27.x.

```bash
# Install Rust; rustup auto-installs the stable toolchain on first use
rustup component add rustfmt clippy
rustup target add wasm32v1-none

# Install Soroban CLI
cargo install soroban-cli

# Clone and build
git clone https://github.com/Meet-hybrid/soroban-forge.git
cd soroban-forge
make build
make test
```

## Code Standards

- Run `cargo fmt --all` before committing.
- Run `cargo clippy --workspace --all-targets -- -D warnings`.
- Write unit tests for all business logic and integration tests for contract interactions.
- Update `CHANGELOG.md` for any user-facing changes.
- Update relevant `docs/` for new features or breaking changes.

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

- `feat(escrow): add refund timeout enforcement`
- `fix(vesting): correct spelling error on vest_message`
- `docs(readme): add troubleshooting section`
- `refactor(shared-utils): simplify ForgeError conversions`
- `test(dao-governance): add proposal execution tests`
- `chore: upgrade soroban-sdk to 21.5.1`
- `perf(subscription): reduce WASM binary size by 12%`
- `ci: add audit workflow`

## Pull Request Process

1. Fork the repository (GitHub **Fork** button), clone your fork, and add
   the upstream remote:

   ```bash
   git clone https://github.com/<your-user>/soroban-forge.git
   cd soroban-forge
   git remote add upstream https://github.com/Meet-hybrid/soroban-forge.git
   git fetch upstream
   git checkout -b my-feature upstream/main
   ```

   Push the branch to your fork and open a Pull Request against
   `Meet-hybrid/soroban-forge:main`. Working in a fork means you never need
   direct push access to the repository; the maintainers review and merge
   your PR.
2. If you've added code that should be tested, add tests.
3. Ensure `make format lint test` passes.
4. Update documentation if needed.
5. Open a Pull Request with a clear title and description.
6. Request review from at least one maintainer.

## Community task workflow

Contributor-facing tasks should use the **Maintainer Task** issue template and
include a single outcome, acceptance criteria, technical context, and
verification commands. A campaign label or complexity label does not itself
guarantee funding; the repository must first be accepted by the campaign
organizers.

If you want to work on an open task, comment with your proposed approach or
apply through the active campaign dashboard and wait for assignment. Do not
start work on a reserved task. Once assigned, create a focused branch and link
the pull request with `Closes #<issue-number>`.

If you cannot finish, tell the maintainer promptly so the issue can be
unassigned and offered to another contributor. Maintainers close an issue as
completed only after the pull request is reviewed, merged, and verified.

## Maintainer Onboarding

- One approval is required by branch protection for contributor pull
  requests; sensitive or non-trivial changes should receive a second
  maintainer review where practical. Repository administrators are exempt,
  so maintainer-authored pull requests are merged through GitHub's admin
  bypass instead of being blocked on a second approver.
- Breaking changes require a discussion issue and 72-hour review period.
- Security fixes follow our Security Policy and are fast-tracked.
- New maintainers should first demonstrate reliable issue triage, review,
  release, and contributor communication before receiving write access.
