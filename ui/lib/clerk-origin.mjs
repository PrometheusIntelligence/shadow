/**
 * The single derivation of Clerk's frontend-API origin from the publishable
 * key. The key's payload IS the frontend host (base64 with a trailing `$`), so
 * this stays in lockstep with whichever Clerk instance is configured:
 *  - pk_test_… -> <slug>.clerk.accounts.dev (third-party dev instance)
 *  - pk_live_… -> clerk.<our-domain> (first-party production instance)
 *
 * Shared by `next.config.mjs` (which needs the origin for the CSP's
 * script-src/connect-src/frame-src) and `app/layout.tsx` (which needs it for
 * the `preconnect` hint), so the two can never drift apart. Plain `.mjs` (not
 * `.ts`) precisely because the config file is ESM JavaScript and cannot import
 * TypeScript.
 *
 * Returns `null` when no key is configured or the payload is not a bare
 * hostname — a malformed env value must never become a URL we emit.
 *
 * @param {string | undefined} publishableKey
 * @returns {string | null}
 */
export function clerkFrontendOrigin(publishableKey) {
  const pk = publishableKey || "";
  const m = /^pk_(?:test|live)_([A-Za-z0-9+/=]+)$/.exec(pk);
  if (!m) return null;
  try {
    const host = Buffer.from(m[1], "base64").toString("utf8").replace(/\$$/, "").trim();
    // Hostname shape only — never let a malformed env value inject CSP tokens
    // or a preconnect target.
    if (/^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/i.test(host)) return `https://${host}`;
  } catch {
    /* fall through to null */
  }
  return null;
}
