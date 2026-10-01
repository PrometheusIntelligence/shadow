import { Suspense } from "react";
import { PageSkeleton } from "@/components/page-skeleton";
import { fetchOpsServer } from "@/lib/ops-data";
import type { Incident } from "@/lib/api";
import { IncidentsClient } from "./incidents-client";
import { OPEN_LIMIT, OPEN_PATH, RESOLVED_PAGE, resolvedPath } from "@/lib/incident-paths";

export default function IncidentsPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <IncidentsData />
    </Suspense>
  );
}

async function IncidentsData() {
  // Open incidents (capped) plus the first page of resolved history — never
  // the whole list (16,612 rows / 10.5 MB on 2026-10-01).
  const [open, resolved] = await Promise.all([
    fetchOpsServer<Incident[]>(OPEN_PATH).catch(() => null),
    fetchOpsServer<Incident[]>(resolvedPath(0)).catch(() => null),
  ]);
  // Cap here too: a backend that ignores the paging params (an older build)
  // must never make this page embed an unbounded list in its HTML again.
  return (
    <IncidentsClient
      initialOpen={open ? open.filter((i) => i.status !== "resolved").slice(0, OPEN_LIMIT) : null}
      initialResolved={resolved ? resolved.filter((i) => i.status === "resolved").slice(0, RESOLVED_PAGE) : null}
    />
  );
}
