import { Suspense } from "react";
import type { Metadata, Viewport } from "next";
import { GeistSans } from "geist/font/sans";
import { GeistMono } from "geist/font/mono";
import { Space_Grotesk, Electrolize } from "next/font/google";
import { ClerkProvider } from "@clerk/nextjs";
import { clerkFrontendOrigin } from "@/lib/clerk-origin.mjs";
import "./globals.css";
import { ChromeTop, ChromeBottom } from "@/components/app-chrome";
import { ThemeProvider } from "@/components/theme-provider";
import { PwaRegister } from "@/components/pwa-register";
import { Toaster } from "@/components/toast";
import { VitalsBeacon } from "@/components/vitals-beacon";

// Still opted out, for a DIFFERENT reason than before (the app-chrome
// usePathname() gap below is fixed — see the Suspense wrapping around
// ChromeTop/ChromeBottom). The remaining blocker is Clerk's own <SignIn>/
// <SignUp> components: they call usePathname() internally
// (@clerk/nextjs's usePathnameWithoutCatchAll), with no Suspense boundary
// of their own, on the /sign-in and /sign-up catch-all routes specifically.
// That call happens INSIDE <ClerkProvider>, which wraps this entire tree —
// there is no seam in this file to add a boundary around a third-party
// component's own internals. Confirmed via `next dev`'s exact error
// attribution (digest: CLIENT_HOOK_DYNAMIC, points at the ClerkProvider
// line). Not fixable from app code; would need either an upstream Clerk fix
// or moving ClerkProvider itself to wrap only the auth routes (a much larger
// restructure of the whole app's auth boundary, out of scope for this pass).
// See: https://nextjs.org/docs/app/guides/migrating-to-cache-components
export const instant = false;

// schema.org JSON-LD so AI search / LLMs (and rich results) can parse what shadw
// is — structured, machine-readable content the AI-search era favors. Kept
// accurate to the product; rendered once in the root so every page carries it.
const STRUCTURED_DATA = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "Organization",
      "@id": "https://shadw.cloud/#org",
      name: "shadw",
      url: "https://shadw.cloud",
      description:
        "shadw is a peer-to-peer cloud for serverless functions, containers, edge routing and durable data over an Iroh QUIC mesh.",
    },
    {
      "@type": "WebSite",
      "@id": "https://shadw.cloud/#site",
      url: "https://shadw.cloud",
      name: "shadw",
      publisher: { "@id": "https://shadw.cloud/#org" },
    },
    {
      "@type": "SoftwareApplication",
      name: "shadw",
      applicationCategory: "DeveloperApplication",
      operatingSystem: "Web",
      offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
      description:
        "Deploy serverless functions, containers, and static sites to a global peer-to-peer edge with instant rollbacks, preview deploys, GitOps, and durable data.",
    },
  ],
};

// Sleek geometric tech typeface for the Shadow brand wordmark + landing.
const display = Space_Grotesk({ subsets: ["latin"], variable: "--font-display", display: "swap" });
// Electrolize — the marketing/landing surface's primary typeface (single 400 weight).
const electrolize = Electrolize({ subsets: ["latin"], weight: "400", variable: "--font-electrolize", display: "swap" });

const SITE_URL = (process.env.NEXT_PUBLIC_SITE_URL || "https://shadw.cloud").replace(/\/$/, "");
const DESCRIPTION =
  "shadw is a peer-to-peer cloud: seamlessly connect, collaborate, and conquer. Serverless functions, containers, edge & durable data over a P2P mesh (Iroh QUIC).";

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: "shadw — Beyond the Edge are Shadows",
    // Public pages set their own titles; this templates them for consistent SEO.
    template: "%s · shadw",
  },
  description: DESCRIPTION,
  applicationName: "shadw",
  manifest: "/manifest.webmanifest",
  alternates: { canonical: "/" },
  // Social + AI-search preview cards (title/description inherit unless a page
  // overrides). OG image is provided by app/opengraph-image.
  openGraph: {
    type: "website",
    siteName: "shadw",
    url: SITE_URL,
    title: "shadw — Beyond the Edge are Shadows",
    description: DESCRIPTION,
  },
  twitter: { card: "summary_large_image", title: "shadw", description: DESCRIPTION },
  // Favicon + icons come from the file-based metadata in app/ (favicon.ico,
  // icon.png, apple-icon.png) so there's a single source of truth.
  appleWebApp: {
    capable: true,
    title: "shadw",
    statusBarStyle: "black-translucent",
  },
};

// Responsive viewport for ALL devices/screen sizes: scale to device width, allow
// user zoom (accessibility — never lock maximumScale), and extend under notches
// (viewport-fit=cover) so mobile safe-areas render correctly. themeColor stays
// theme-aware for the browser/installed-app UI.
export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  viewportFit: "cover",
  themeColor: [
    { media: "(prefers-color-scheme: dark)", color: "#000000" },
    { media: "(prefers-color-scheme: light)", color: "#ffffff" },
  ],
};

// NOTE: the root layout is intentionally NOT `force-dynamic`. All auth-dependent
// chrome (TopNav/Footer via <SignedIn>) and the home landing↔dashboard flip are
// CLIENT components that hydrate to the correct state, and NO page reads server
// auth (`auth()`/`cookies()`/`headers()`), so public pages can be prerendered as
// static/ISR (huge mobile win) and dashboard pages ship a fast static shell that
// fetches per-user data after hydration. Only the home route ('/') keeps
// `force-dynamic` (its whole-page content flip would otherwise flash).

// Clerk is enabled when a publishable key is present; otherwise the app runs in
// local mode (no auth) so it still works without keys.
const clerkEnabled = !!process.env.NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY;
// Preconnect target for clerk-js (see the <link> in the tree below). Null
// whenever no key is configured or its payload isn't a bare hostname, so a
// malformed env value can never become a URL we emit.
const clerkOrigin = clerkFrontendOrigin(process.env.NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY);

// Origins Clerk is allowed to redirect back to after sign-in / OAuth — i.e. the
// app's callback URLs. This lets login work both locally and on the public
// platform domain. Override/extend via NEXT_PUBLIC_ALLOWED_ORIGINS
// (comma-separated); the defaults cover the common local ports + shadw.cloud.
// `www.shadw.cloud` serves the SAME app (live-verified: it answers 200 with
// the dashboard, no redirect to the apex), so it must be allow-listed too —
// otherwise Clerk refuses every redirect_url on the www origin and a sign-in
// there bounces back to where it started.
const allowedRedirectOrigins = (
  process.env.NEXT_PUBLIC_ALLOWED_ORIGINS ||
  "http://localhost:3000,http://localhost:3002,https://shadw.cloud,https://www.shadw.cloud"
)
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const tree = (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${GeistSans.variable} ${GeistMono.variable} ${display.variable} ${electrolize.variable}`}
    >
      <head>
        {/* clerk-js is the ONE third-party request on every page's critical
            path (ClerkProvider loads
            <origin>/npm/@clerk/clerk-js@5/dist/clerk.browser.js), including the
            public marketing landing it has nothing to do with. Warm the socket
            while the HTML is still parsing so that fetch doesn't pay DNS + TCP
            + TLS first — measured on the fleet, a TLS handshake to a fresh
            origin costs 60-500 ms, the same order as the landing page's whole
            TTFB. A real <link> element rather than react-dom's `preconnect()`:
            that emits an HTTP `Link` header, which only Chrome honours, whereas
            this tag is honoured by every browser. Deferring clerk-js entirely
            would mean not mounting ClerkProvider on public routes — a much
            larger auth-boundary restructure (see the instant=false note above),
            so this is the cheap half of the win. */}
        {clerkEnabled && clerkOrigin ? (
          <link rel="preconnect" href={clerkOrigin} crossOrigin="anonymous" />
        ) : null}
      </head>
      <body className="flex min-h-screen flex-col bg-bg font-sans text-fg antialiased">
        <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(STRUCTURED_DATA) }} />
        <PwaRegister />
        <VitalsBeacon />
        <Toaster />
        <ThemeProvider>
          {/* Dashboard chrome (top nav + footer + overlays) is auth-gated in a
              CLIENT component so it reacts to client-side login/logout — the
              signed-out landing renders its own full-bleed nav/footer. See
              `app-chrome.tsx` for why this must not live in the server layout.
              Suspense-wrapped: TopNav/Footer read usePathname(), a client hook
              that needs a boundary under Cache Components to prerender the
              shell (see instant=false's removal). fallback={null} is exactly
              correct — both components already conditionally render nothing
              until auth settles, so this introduces no visible change. */}
          <Suspense fallback={null}>
            <ChromeTop />
          </Suspense>
          <main className="mx-auto w-full max-w-[1400px] flex-1 px-4 py-8 sm:px-6">{children}</main>
          <Suspense fallback={null}>
            <ChromeBottom />
          </Suspense>
        </ThemeProvider>
      </body>
    </html>
  );
  return clerkEnabled ? (
    <ClerkProvider allowedRedirectOrigins={allowedRedirectOrigins}>{tree}</ClerkProvider>
  ) : (
    tree
  );
}
