"use client";

import dynamic from "next/dynamic";
import Link from "next/link";
import { Landing } from "@/components/landing";
import { useSettledAuth, resetAuthSettle } from "@/lib/auth-settle";

// The dashboard is its OWN chunk, fetched only once auth resolves to
// signed-in. `/` is BOTH the public marketing landing and the authenticated
// dashboard home, and a first-time visitor is overwhelmingly signed out —
// statically importing `Dashboard` made every anonymous landing download the
// whole dashboard tree (its polling hooks, table/card views and icon set) on
// the critical path. `ssr: false` because this subtree is client-only by
// construction: auth resolves on the client (see the note on HomeClient), so
// there is nothing for the server to prerender here.
const Dashboard = dynamic(() => import("./home-dashboard").then((m) => m.Dashboard), {
  ssr: false,
});

const clerkEnabled = !!process.env.NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY;

// The root route body (client): the public Shadow landing for signed-out
// visitors, the dashboard for signed-in users. In local (no-Clerk) mode it's just
// the dashboard. Rendered by the force-dynamic server shell in page.tsx. Auth is
// resolved CLIENT-side only (the layout's ClerkProvider has no `dynamic` prop,
// so SSR carries no request auth): server HTML and the first client render both
// show NEITHER branch — there is no hydration mismatch, just one client-side
// commit when clerk-js settles.
//
// GUARDED, not raw `<SignedIn>/<SignedOut>`: this route flips the ENTIRE page
// between two different applications, so it renders from `useSettledAuth()` —
// a committed, debounced, flip-BOUNDED view of Clerk's auth state (see
// lib/auth-settle.ts). A flapping auth signal (the mobile/ITP third-party
// cookie handshake failure) therefore cannot thrash landing↔dashboard: fast
// flaps are absorbed with zero paints, slow oscillation is cut off after a
// bounded number of flips by a stable, honest degraded state with a working
// sign-in affordance. The first loaded sample commits with no debounce, so the
// normal case paints as fast as the old control components did.
export function HomeClient() {
  if (!clerkEnabled) return <Dashboard />;
  // Separate subtree so `useSettledAuth` (which needs ClerkProvider) is never
  // called in local/no-Clerk mode — same split the old control components had.
  return <GuardedHome />;
}

function GuardedHome() {
  const view = useSettledAuth();
  if (view === "in") return <Dashboard />;
  if (view === "degraded") return <AuthDegraded />;
  // "out" AND the not-yet-resolved state both render the Landing.
  //
  // This used to render a bare spinner while auth was unresolved, which made
  // the prerendered `/` an essentially EMPTY page (24 KB: title + an
  // `aria-busy` placeholder, no hero, no globe) and put the entire landing
  // behind a chain of HTML -> JS bundle -> hydration -> Clerk's THIRD-PARTY
  // script resolving -> render. On a slow connection that last hop is an
  // indeterminate round trip to accounts.dev, and the guard would wait up to
  // RESOLVE_TIMEOUT_MS (8s) before settling — which is exactly the reported
  // "the planet loads inconsistently, especially on slow networks".
  //
  // The Landing is identical for every visitor (no per-request data), so it is
  // safe to show before auth is known and it can therefore be PRERENDERED:
  // the hero, the CTAs and the globe are now in the initial HTML. Only the
  // signed-in Dashboard needs the auth decision, and the guard's hysteresis
  // plus flip bound still apply to that one transition, so an oscillating
  // signal cannot thrash landing<->dashboard (the failure this guard exists
  // for). The cost is that a signed-in user may see the landing briefly
  // before the dashboard — strictly better than everyone seeing a blank page.
  return <Landing />;
}

/** Terminal state after the guard's flip bound tripped: the auth signal on
 *  this browser is flapping (e.g. iOS blocking the third-party session
 *  handshake), so instead of endlessly re-painting we settle on the stable
 *  signed-out Landing plus an honest, actionable explanation. "Try again" is
 *  the ONLY way out (explicit user action — automatic retry would re-enter
 *  the loop this state exists to end). */
function AuthDegraded() {
  return (
    <>
      <Landing />
      <div
        role="alert"
        className="fixed inset-x-0 bottom-0 z-[200] border-t border-amber-500/40 bg-[#141519]/95 px-4 py-3 text-sm text-zinc-200 backdrop-blur"
      >
        <div className="mx-auto flex max-w-3xl flex-wrap items-center justify-center gap-x-4 gap-y-2 text-center">
          <span>
            We couldn&apos;t keep you signed in on this browser — it may be blocking the
            cookies sign-in needs (common in private browsing on iPhone). You can keep
            browsing signed out, or try again.
          </span>
          <span className="flex shrink-0 items-center gap-4">
            <Link href="/sign-in" className="font-semibold text-white underline underline-offset-2">
              Sign in
            </Link>
            <button
              type="button"
              onClick={resetAuthSettle}
              className="font-semibold text-white underline underline-offset-2"
            >
              Try again
            </button>
          </span>
        </div>
      </div>
    </>
  );
}
