"use client";

import { GLOBE_TRACERS_SVG } from "@/components/globe-tracers";

/**
 * The hero globe: a raster globe image with the animated tracer overlay drawn
 * on top. Both halves are DECLARATIVE — no fetch, no effect, no JavaScript —
 * so the globe is discovered and fetched by the browser while it parses the
 * HTML, in parallel with the JS bundle, instead of after hydration.
 *
 * The reason this used to be a `fetch()` inside a `useEffect` was that the
 * artwork shipped as one 906,804-byte `/globe-wireframe.svg` whose 2048x762
 * globe raster was embedded as a base64 `data:image/png` — 904,850 of its
 * bytes. That made it 886 KB on the wire (gzip could not help: the PNG is
 * already compressed and base64 inflates it a further 33%), slow enough that
 * it visibly arrived late on a slow connection, and it was requested only
 * once React had mounted. Worst of all, the failure path was `.catch(() => {})`
 * — a failed or timed-out fetch left this region permanently blank with no
 * placeholder and no signal, which is the "loads inconsistently" symptom.
 *
 * Now: the raster is its own cacheable image (WebP lossless, 405 KB, with a
 * PNG fallback) and the ~21 KB of tracer markup ships with the component. The
 * tracers are in the live document so their SMIL timeline animates, and they
 * paint immediately — so even before the raster arrives the hero shows the
 * globe's light routes rather than an empty box.
 */
export function GlobeWireframe({ className }: { className?: string }) {
  return (
    <div className={className}>
      {/* Fixed aspect-ratio box → both layers have a definite containing
          block, so nothing can render at the artwork's intrinsic 2048px and
          shift the layout. */}
      <div className="relative w-full overflow-hidden" style={{ aspectRatio: "2048 / 762" }}>
        {/* Responsive sources: the artwork is 2048 wide but the box is
            `max-w-6xl` (1152px minus padding), so a phone was downloading
            405 KB for a ~390 CSS-px image. All three widths are WebP LOSSLESS
            (verified pixel-identical to the original raster), so this costs no
            fidelity — only bytes the device cannot resolve anyway. 1024w is
            127 KB, 1536w is 275 KB, 2048w is 405 KB.
            `fetchPriority="high"`: hero-critical, not a late-discovered image. */}
        <picture>
          <source
            type="image/webp"
            sizes="(max-width: 1200px) 100vw, 1152px"
            srcSet="/globe-wireframe-1024.webp 1024w, /globe-wireframe-1536.webp 1536w, /globe-wireframe.webp 2048w"
          />
          <img
            src="/globe-wireframe.png"
            alt=""
            aria-hidden="true"
            width={2048}
            height={762}
            fetchPriority="high"
            decoding="async"
            className="pointer-events-none absolute inset-0 block h-full w-full select-none"
          />
        </picture>
        {/* SMIL cannot be paused from CSS, so the tracer layer is simply not
            shown when the visitor prefers reduced motion — the static globe
            underneath is the whole graphic either way. `motion-reduce:hidden`
            keeps this in CSS (no JS, still prerendered) rather than gating the
            markup on a client-side media query, which would put the globe back
            behind hydration. */}
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 select-none motion-reduce:hidden [&>svg]:block [&>svg]:h-full [&>svg]:w-full"
          dangerouslySetInnerHTML={{ __html: GLOBE_TRACERS_SVG }}
        />
      </div>
    </div>
  );
}
