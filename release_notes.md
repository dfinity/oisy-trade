- SHA-256 hash: `${OISY_TRADE_CANISTER_WASM_GZ_SHA256}`

## Deployments

| Type                | Canister ID                                                                                                  | Deployed? |
|---------------------|--------------------------------------------------------------------------------------------------------------|-----------|
| :rocket: Production | [`sy2xe-miaaa-aaaar-qb7sq-cai`](https://dashboard.internetcomputer.org/canister/sy2xe-miaaa-aaaar-qb7sq-cai) | :x:       |
| :test_tube: Staging | [`proc5-daaaa-aaaar-qb5va-cai`](https://dashboard.internetcomputer.org/canister/proc5-daaaa-aaaar-qb5va-cai) | :x:       |

## What's Changed

${CHANGELOG}

## Reproducible build

The attached `oisy_trade_canister.wasm.gz` is built reproducibly. To verify the hash matches:

```bash
git checkout ${RELEASE_TAG}
just docker-build
sha256sum wasms/oisy_trade_canister.wasm.gz
```
