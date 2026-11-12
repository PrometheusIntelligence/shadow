# @openedge/vercel-sdk

An **independent client for the OpenEdge platform API** whose constructor and calling
convention mirror the upstream [`@vercel/sdk`](https://github.com/vercel/sdk), so code
written against that shape can be repointed at **your OpenEdge deployment** and
authenticate with an **OpenEdge platform key (`hive_…`)** instead of a Vercel access token.

> **Provenance.** This package is an independent implementation of the public `@vercel/sdk`
> constructor shape. It is **not** a copy or derivative of the upstream source: compared with
> `@vercel/sdk@1.28.62` (Apache-2.0, 1,175 generated TypeScript files), only two trivial
> lines coincide (`export class VercelError extends Error {` and its `this.name` assignment),
> and it has zero runtime dependencies. It covers only the `integrations` namespace;
> it is **not** a drop-in replacement for the full upstream API surface.
> "Vercel" is a trademark of Vercel Inc., used here only to describe interface
> compatibility; this package is not affiliated with or endorsed by Vercel Inc.

It keeps the upstream constructor ergonomics — construct a `Vercel` client with `{ serverURL, bearerToken }`
and call namespaced sub-APIs — so code that only constructs the client repoints with a one-line change:

```diff
- import { Vercel } from "@vercel/sdk";
+ import { Vercel } from "@openedge/vercel-sdk";

  const vercel = new Vercel({
-   bearerToken: process.env.VERCEL_TOKEN,
+   serverURL: process.env.OPENEDGE_API_URL,   // your OpenEdge deployment's API
+   bearerToken: process.env.OPENEDGE_API_KEY, // a hive_… platform key (Settings → API Keys)
  });
```

## What's different from upstream

| | upstream `@vercel/sdk` | this package |
|---|---|---|
| `serverURL` | `https://api.vercel.com` | **your OpenEdge API** (env `OPENEDGE_API_URL`, default `http://127.0.0.1:8786`) |
| `bearerToken` | Vercel token | **`hive_…` platform key** (env `OPENEDGE_API_KEY`) — its team scopes every call |
| team scoping | `teamId`/`slug` query params | derived from the key; optional `team` → `x-hive-team` |
| `integrations` namespace | n/a | lists Composio-linked integrations + their credentials/env |

## Integrations

When you connect an integration on the **Integrations** tab (OAuth via Composio), the platform
registers it as a team-scoped resource and auto-injects its credentials as deployment env vars.
This SDK reads those back:

```ts
import { Vercel } from "@openedge/vercel-sdk";

const vercel = new Vercel({
  serverURL: "https://app.example.com",   // your OpenEdge API origin
  bearerToken: process.env.OPENEDGE_API_KEY, // hive_…
});

// All connected integrations (redacted).
const integrations = await vercel.integrations.list();
// → [{ provider: "stripe", name: "Stripe", envKeys: ["STRIPE_API_KEY"], … }]

// One integration's secret credentials + env values.
const stripe = await vercel.integrations.byProvider("stripe");
if (stripe) {
  const { credentials, env } = await vercel.integrations.credentials(stripe.id);
  // credentials.api_key, env.STRIPE_API_KEY, …
}

// Convenience: every integration's env var, merged (same set injected into deployments).
const env = await vercel.integrations.env();
```

In a deployment you usually don't need the SDK at all — the env vars are already injected
(`STRIPE_API_KEY`, etc.). The SDK is for fetching them on demand, in other environments, or
when you want the live token rather than the snapshot.

## API

- `new Vercel({ serverURL?, bearerToken?, team?, fetch? })`
- `vercel.integrations.list(): Promise<Integration[]>`
- `vercel.integrations.get(id): Promise<Integration>`
- `vercel.integrations.byProvider(slug): Promise<Integration | null>`
- `vercel.integrations.credentials(id): Promise<IntegrationCredentials>`
- `vercel.integrations.env(): Promise<Record<string,string>>`

Zero runtime dependencies (uses global `fetch`). Node 18+, edge runtimes, Deno, Bun, browser.
