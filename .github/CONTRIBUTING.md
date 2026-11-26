# Contributing

Thank you for your interest in contributing to OISY TRADE! Bug reports, suggestions, and pull requests are all welcome.

## Reporting issues

- **Security vulnerabilities:** do **not** open a public issue. Follow the instructions in [SECURITY.md](/SECURITY.md) instead.
- **Bugs and feature requests:** open a [GitHub issue](https://github.com/dfinity/oisy-trade/issues) with as much detail as possible (version or commit, steps to reproduce, expected vs. actual behavior).
- **General questions:** ask on the [DFINITY developer forum](https://forum.dfinity.org/).

## Submitting a pull request

1. **Sign the CLA.** All external contributors must agree to the [DFINITY Contributor License Agreement](https://github.com/dfinity/cla/blob/main/CLA.md). A bot will guide you through this on your first pull request.
2. **Discuss larger changes first.** For anything beyond a small fix, please open an issue first so we can agree on the approach before you invest time in it.
3. **Fork and branch.** Fork the repository and create a branch for your change. Keep each pull request focused on a single fix or feature.
4. **Build and test locally.** See the [Development](/README.md#development) section of the README. At a minimum, run:

   ```bash
   just lint
   just unit-tests
   ```

   If you changed the canister, also update the benchmark results, which CI checks with canbench 0.4.1:

   ```bash
   cargo install --version 0.4.1 --locked canbench
   just bench-check   # then commit canister/canbench_results.yml
   ```

5. **Use a conventional PR title.** CI enforces titles of the form `type(scope): description`, where `type` is one of `feat`, `fix`, `chore`, `build`, `ci`, `docs`, `style`, `refactor`, `perf`, or `test`, e.g. `fix(canister): reject zero-quantity orders`. The scope is optional and may only contain letters, digits, `-`, `_`, and `,`, so use e.g. `docs(readme)` rather than `docs(README.md)`. Mark breaking changes with `!`, e.g. `feat(api)!: ...`. A breaking change to the Candid interface (`canister/oisy_trade.did`) additionally needs the `candid-breaking-change` label, which only maintainers can add; a maintainer then re-runs the `candid-backward-compat` check.
6. **Open the pull request** against `main` and describe what it changes and why.

All pull requests are reviewed by the code owners in [CODEOWNERS](/.github/CODEOWNERS) and must pass CI before they can be merged. CI workflows for pull requests from forks only run after a maintainer approves them.

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](/LICENSE).
