# @soroban-forge/escrow-client

Generated TypeScript client for the **Soroban Forge escrow contract**, live on
Stellar testnet:

> `CC227UDF6WBLRTOKKVRIJN7BGSBK67ZGV6IDARJ2AMATGSQ7UZNBZHSB`

The client is generated from the deployed contract's ABI by the Stellar CLI, so
every method is fully typed and carries the doc comments from the contract
source.

## Install

```bash
npm install
npm run build   # emits dist/
```

## Usage

```ts
import { SorobanForgeEscrowClient, networks } from "@soroban-forge/escrow-client";

const client = new SorobanForgeEscrowClient({
  ...networks.testnet,
  rpcUrl: "https://soroban-testnet.stellar.org", // or your own RPC
});

// Every contract method is available, typed, with `try*` variants:
const id = await client.createEscrow({
  buyer: "GCTEIX…",
  seller: "GCGPZ3…",
  arbiter: "GCR7CB…",
  token: "CBJQ53…",
  amount: 500n,
  timeout: 86_400n,
});

await client.deposit({ escrow_id: id.result });
await client.release({ escrow_id: id.result });
await client.dispute({ escrow_id: id.result, claimant: seller });
await client.resolve({ escrow_id: id.result, in_favor_of_seller: true });
await client.getStatus({ escrow_id: id.result });
await client.touchTtl({ escrow_id: id.result }); // permissionless keeper
```

The `networks` export carries the embedded `contractId` and network passphrase;
pass your own `rpcUrl`. Signers/wallets are supplied per call via `MethodOptions`
(`sign`, `simulate`, etc.) — see the
[stellar-sdk contract client docs](https://stellar.github.io/js-stellar-sdk/).

## Provenance

Regenerate after any contract interface change:

```bash
stellar contract bindings typescript \
  --contract-id CC227UDF6WBLRTOKKVRIJN7BGSBK67ZGV6IDARJ2AMATGSQ7UZNBZHSB \
  --network testnet \
  --output-dir packages/typescript-sdk --overwrite
```

This package replaces the v0.1.0 console-log placeholder SDK. The contract
itself, its testnet receipt round, and the conservation property are documented
in the [repository README](https://github.com/Meet-hybrid/soroban-forge).
