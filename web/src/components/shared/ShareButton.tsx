import { useState, useCallback } from "preact/hooks";
import { createShare } from "src/api/client.ts";

interface ShareButtonProps {
  readonly getState: () => Record<string, unknown>;
}

export function ShareButton({ getState }: ShareButtonProps) {
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleShare = useCallback(async () => {
    setLoading(true);
    setCopied(false);
    try {
      const state = getState();
      const response = await createShare(state);
      setShareUrl(response.url);
    } catch {
      setShareUrl(null);
    } finally {
      setLoading(false);
    }
  }, [getState]);

  const handleCopy = useCallback(async () => {
    if (shareUrl === null) return;
    try {
      await navigator.clipboard.writeText(shareUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API unavailable -- select the text instead
    }
  }, [shareUrl]);

  return (
    <div class="share-button-group">
      <button
        class="btn btn-secondary btn-sm"
        onClick={handleShare}
        disabled={loading}
        title="Create a shareable link"
      >
        {loading ? "Sharing..." : "Share"}
      </button>
      {shareUrl !== null && (
        <div class="share-url-row">
          <input
            class="share-url-input"
            type="text"
            value={shareUrl}
            readOnly
            onClick={(e) =>
              (e.target as HTMLInputElement).select()
            }
          />
          <button
            class="btn btn-sm"
            onClick={handleCopy}
          >
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      )}
    </div>
  );
}
