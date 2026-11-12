# Third-Party Notices

Shadow includes or depends on the third-party software listed below. Each component
is licensed under its own terms, as stated here and in the license files shipped next to it. The
proprietary terms in `LICENSE.md` cover only the Licensor's original work (see `LICENSE.md` §7).

This inventory was produced with ScanCode Toolkit 33.0.0rc1 (`scancode -clpi`) over the tracked
files at commit `34f7492`, plus `cargo metadata --locked` and the npm lockfiles. Re-run both
before each release.

## Source code copied into this repository

| Path | Upstream | Version / commit | License | Modified? | License file in tree |
|---|---|---|---|---|---|
| `ui/vendor/workflow-web/` | [vercel/workflow](https://github.com/vercel/workflow) (`@workflow/web`, `@workflow/web-shared`, `@workflow/world`) | commit `1225258b5da66be40749c17d12c4e24d32197aa2` (web 5.0.0-beta.36, world 5.0.0-beta.22) | Apache-2.0, Copyright Vercel Inc. | No. Byte-identical to upstream at that commit (verified 2026-11-12). Provenance is in `VENDOR-INFO.json`. | `ui/vendor/workflow-web/LICENSE.md` |
| `vendor/iroh/` | [n0-computer/iroh](https://github.com/n0-computer/iroh) | 1.0.2 (crates.io checksum in `CHANGES.md`) | MIT OR Apache-2.0, Copyright N0, INC.; parts BSD-3-Clause (Tailscale) | **Yes.** 7 files under `src/` are patched. Each carries a modification notice, and the patches are described in `vendor/iroh/CHANGES.md`. | `LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE-BSD3` |
| `vendor/guardian-db/` | [wmaslonek/guardian-db](https://github.com/wmaslonek/guardian-db) | 0.18.0 | MIT OR Apache-2.0 | See upstream history | `LICENSE-MIT`, `LICENSE-APACHE` |
| `vendor/cr-sqlite/` | [vlcn-io/cr-sqlite](https://github.com/vlcn-io/cr-sqlite) | vendored tree | MIT, Copyright One Law LLC; bundled SQLite is public domain ("blessing") | — | `vendor/cr-sqlite/LICENSE` |
| `vendor/ws_stream_wasm/` | [najamelan/ws_stream_wasm](https://github.com/najamelan/ws_stream_wasm) | vendored tree | Unlicense | — | `vendor/ws_stream_wasm/LICENSE` |
| `crates/hive-browser/www/sqlite/wa-sqlite/`, `ui/public/browser-node/sqlite/wa-sqlite/` | [rhashimoto/wa-sqlite](https://github.com/rhashimoto/wa-sqlite) | `VFS.js` = upstream `a110948`, `sqlite-constants.js` = `c8d0b3d`, `examples/AccessHandlePoolVFS.js` = `cf05819`; `sqlite-api.js` differs from every upstream revision checked (derived) | MIT, Copyright Roy T. Hashimoto | `sqlite-api.js` only | `LICENSE` in each copy |
| `vendor/node-worker/` and its build in `ui/public/browser-node/node-worker/` | node-worker (Node.js-in-a-Worker runtime; appears to originate from the Puter project) | vendored tree with local `patches/` and `node-patches/` | **AGPL-3.0-only** | Yes (patches in tree) | `vendor/node-worker/LICENSE`, plus `LICENSE` and `NOTICE.txt` in the served build directory |
| `crsqlite-sync.wasm` (same two directories) | Built from `vendor/cr-sqlite` + SQLite + wa-sqlite | — | MIT + public domain | Build output | see the rows above |

## AGPL-3.0 component: legal review required

`node-worker` is the only strong-copyleft code in this repository. It is served to every browser
that uses the browser-node lane, and it is modified (the patches are in the tree). Under AGPL-3.0
§§ 4–6 and § 13, everyone who receives it or interacts with it over the network must be able to
get the Corresponding Source of the modified version, under AGPL-3.0. The repository is public,
and the served directory now carries the license and a pointer to the source, which covers the
notice side.

Two questions need a lawyer before Shadow is sold or relicensed commercially:

1. Whether the platform JavaScript that loads node-worker in the same page
   (`node-worker-host.js`, `worker-function-runtime.js`, `peer-mesh*.js`, `hive_browser.js`) is a
   separate work or a work based on it.
2. Whether the upstream copyright holders' terms permit the intended commercial distribution.

If the answer is unfavourable, the options are to replace or isolate the component, or to obtain
a commercial license from its authors.

The DevHub / OpenEdge fork (`L0_devhub_deploy`) does **not** contain node-worker.

## Packages that are not affiliated with Vercel

`packages/vercel-sdk` (`@openedge/vercel-sdk`) is an **independent implementation** of the
`@vercel/sdk` constructor shape. It contains no upstream source. Compared with
`@vercel/sdk@1.28.62` (Apache-2.0), only two trivial lines coincide. The package is MIT. "Vercel"
is a trademark of Vercel Inc. and is used only to describe interface compatibility.

`ARCHITECTURE.md` describes an independent Rust implementation of the architecture Vercel
published in a public blog post. ScanCode found no Vercel copyright statements outside
`ui/vendor/workflow-web/`.

## Rust dependencies (`Cargo.lock`, 776 registry crates)

| License | Count |
|---|---|
| MIT OR Apache-2.0 (in its various spellings) | ~477 |
| MIT | 169 |
| Unicode-3.0 | 18 |
| Apache-2.0 (incl. WITH LLVM-exception options) | ~30 |
| BSD-2/3-Clause, ISC, Zlib, Unlicense, CC0, BSL-1.0 | remainder |

Copyleft notes, all of which are acceptable for proprietary distribution:

- `attohttpc` 0.30.1 is MPL-2.0. MPL is file-level copyleft: unmodified use is fine, and its
  source files must stay MPL if they are ever changed.
- `self_cell` (Apache-2.0 OR GPL-2.0-only), `r-efi` (MIT OR Apache-2.0 OR LGPL-2.1+) and
  `htmlescape` (Apache-2.0 / MIT / MPL-2.0) are used under their permissive option.
- No crate is GPL-, AGPL-, SSPL- or BUSL-only.

## Dashboard npm dependencies (`ui/package-lock.json`, 608 packages)

| License | Count |
|---|---|
| MIT | 482 |
| Apache-2.0 | 46 |
| ISC | 44 |
| LGPL-3.0-or-later | 10 |
| BSD-2/3-Clause | 13 |
| Apache-2.0 AND LGPL-3.0-or-later | 4 |
| MPL-2.0, CC-BY-4.0, BlueOak-1.0.0, Unlicense, OFL, CC0, 0BSD, Python-2.0 | 1 each |

The LGPL entries are all `@img/sharp-libvips-*` / `@img/sharp-*` (the prebuilt libvips used by
`sharp`, which Next.js uses for image optimisation). libvips is dynamically loaded and unmodified,
which LGPL-3.0 allows in proprietary software. Keep its license text with any redistribution of
`node_modules`, and do not statically link or modify it.

## Docs that mention AGPL

`docs/browser-node-proposal.md` discusses AGPL-3.0 projects that were studied for the browser-node
lane. Lagon (archived, AGPL-3.0) is named there only as an interface reference for a clean-room
reimplementation. No Lagon source is in the tree; the only code reference is one doc comment in
`crates/hive-browser/src/lib.rs` describing a "Lagon-shaped" request. The AGPL code that *is* in
the tree is `node-worker`, described above.
