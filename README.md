# DEX

> Order book DEX on the [Internet Computer](https://internetcomputer.org/).

## Architecture :building_construction:

See the [design document](docs/design.md) for the high-level architecture.

## Deployment :rocket:

| Environment | Canister ID | Dashboard |
|-------------|-------------|-----------|
| Staging | `proc5-daaaa-aaaar-qb5va-cai` | [View](https://dashboard.internetcomputer.org/canister/proc5-daaaa-aaaar-qb5va-cai) |

## Development :hammer_and_wrench:

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)
- [icp CLI](https://cli.internetcomputer.org/)

### Build

List all available recipes with `just`.

| Command | Description |
|---------|-------------|
| `just lint` | Run linter |
| `just build` | Build the canister WASM |
| `just ci` | Run all checks, build, and tests |

### Deploy to staging

Requires the [icp CLI](https://cli.internetcomputer.org/) with an identity that has deploy permissions.
By default the `hsm` identity is used; override with `just deploy <identity>`.

```bash
just deploy
```
