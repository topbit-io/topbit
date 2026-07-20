# TopBit: GambleFi on Solana. v1 On-Chain Program Source (Verification and Audit Only)

This repository contains the exact Rust/Anchor source for the seven deployed
TopBit v1 Solana programs. It is published **solely to permit independent
verification and audit** of the on-chain bytecode. It is **not** open source.
See [`LICENSE`](./LICENSE) for the full terms. No right to use, deploy, run,
host, distribute, modify, or create derivative works is granted.

## Deployed programs (Solana mainnet-beta)

| Program (library name) | Program ID |
|---|---|
| `tlp_provider_vault` | `CtB3xQvmUGZtFRALhmkhhissargBJ51WPLCDFhGVy6Lx` |
| `staking` | `2n2puiEN8BbMMEtq387b6HKR2trvKY9rK5uM82Ht2Vtc` |
| `etop_escrow` | `Aa9CbHs9yDt52x4jfyQfeyb6R7nYxUjABvYbbcgRMuro` |
| `yield_escrow` | `85b3FfAzz3akfnH7NPCqR4Pjna45N3N6e6MvPsxABJ6n` |
| `swap_router` | `9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR` |
| `affiliate_registry` | `GcLnquNequt8UwNigWDzyfA2DpeeTMnQnyUbHxHL8cfC` |
| `sovereign_registry` | `14ndgn3yKuD4Zi3ozBt7Fo4cYzUuYDAZrTn15wT3rFC2` |

The repository root is the Anchor workspace root (verification `--mount-path` is `.`).

## Reproducible build toolchain

Builds are reproduced inside the pinned Solana Foundation verifiable-build image,
which fixes the compiler and platform-tools versions (platform-tools v1.52):

```
solanafoundation/solana-verifiable-build:3.1.12
```

## Verify against on-chain bytecode

With [`solana-verify`](https://github.com/Ellipsis-Labs/solana-verifiable-build)
installed and Docker running, verify any program directly from this repository
(replace `<REPO_URL>` with this repository's URL):

```bash
solana-verify verify-from-repo \
  --base-image solanafoundation/solana-verifiable-build:3.1.12 \
  --mount-path . \
  --library-name tlp_provider_vault \
  --program-id CtB3xQvmUGZtFRALhmkhhissargBJ51WPLCDFhGVy6Lx \
  <REPO_URL>
```

Repeat with each `--library-name` / `--program-id` pair from the table above.

## Reproduce the build locally

From the repository root:

```bash
solana-verify build \
  --base-image solanafoundation/solana-verifiable-build:3.1.12 \
  --library-name <library_name>

sha256sum target/deploy/<library_name>.so
```

The raw `sha256sum` of each locally rebuilt `target/deploy/<library>.so` is:

```
tlp_provider_vault  5bccb861874d6256d1fbd316bec5da5d34ad15aceafad2346eebb6c7caa7f23a
staking             7ce80871e7d94252f1ec8d9091434ac0f6d86dbb7d6791df00a1750bf949bf3c
etop_escrow         6c85caf6c2b5a711add09a35560b1809df1cfe1e717129cdd561462db08e9d45
yield_escrow        a706e33775b563b5e70caac7d55aad7ed071cb65a1ccf64f7d2ab878bea532c6
swap_router         f6311101341c38be09456906463b28588bb9c7bec1a79ce80f6b2adc3411a3d3
affiliate_registry  817ab32b0d5693f7a410ab205a43bac984f95393193684dc6070c15218121dac
sovereign_registry  e1e37c5357ae1386814c6aa38fa71a9ec52b65b50400e013bdcf6a1731d47248
```

`Cargo.lock` is committed to pin the exact dependency versions used for the
deployed builds; `solana-verify build` passes `--locked`.

## License

Source-available for verification and audit only. See [`LICENSE`](./LICENSE).
This is not, and will not become, an open-source license.
