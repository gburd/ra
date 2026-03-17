import { useState, useEffect } from "preact/hooks";
import { route } from "preact-router";
import { loadShare } from "src/api/client.ts";

interface SharePageProps {
  readonly path: string;
  readonly id?: string;
}

/**
 * Loads a shared state by ID and redirects to the
 * appropriate page with the shared content pre-filled.
 */
export function SharePage({ id }: SharePageProps) {
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (id === undefined) {
      setError("No share ID provided.");
      setLoading(false);
      return;
    }

    let cancelled = false;

    async function load(shareId: string): Promise<void> {
      try {
        const data = await loadShare(shareId);
        if (cancelled) return;

        if (typeof data["sql"] === "string") {
          const sql = encodeURIComponent(data["sql"]);
          const db = typeof data["database"] === "string"
            ? data["database"]
            : "sqlite";
          route(`/editor?sql=${sql}&db=${db}`);
          return;
        }

        setError("Unable to load shared content.");
      } catch (err) {
        if (!cancelled) {
          setError(
            err instanceof Error ? err.message : String(err),
          );
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void load(id);

    return () => {
      cancelled = true;
    };
  }, [id]);

  if (loading) {
    return (
      <div class="share-page">
        <div class="loading-indicator">Loading shared content...</div>
      </div>
    );
  }

  if (error !== null) {
    return (
      <div class="share-page">
        <div class="error-banner">{error}</div>
        <p style={{ marginTop: "12px" }}>
          <a href="/">Return home</a>
        </p>
      </div>
    );
  }

  return null;
}
