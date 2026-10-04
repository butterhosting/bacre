import type { Archives } from "@/models/Archives";
import { useState } from "react";
import { ArchiveClient } from "../clients/ArchiveClient";
import { useRegistry } from "../hooks/useRegistry";
import { Button } from "./Button";

type Props = {
  staged: Archives.Staged;
  onDiscarded: () => Promise<void>;
  onClose: () => void;
};
export function DiscardDialog({ staged, onDiscarded, onClose }: Props) {
  const archiveClient = useRegistry(ArchiveClient);
  const [discarding, setDiscarding] = useState(false);
  const [error, setError] = useState<string>();

  async function discard() {
    setDiscarding(true);
    setError(undefined);
    try {
      await archiveClient.discard(staged);
      await onDiscarded();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setDiscarding(false);
    }
  }

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-[#6b6259]/80 p-3 md:p-6" onClick={onClose}>
      <div
        role="dialog"
        aria-modal
        aria-label={`Discard ${staged.service}`}
        className="max-h-full w-[520px] max-w-full overflow-y-auto rounded-[14px] border border-c-line bg-c-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-6 pt-5 pb-4.5">
          <h2 className="font-display text-2xl font-semibold">Discard the download?</h2>
        </div>

        <div className="flex items-center justify-between gap-4 border-t border-c-line bg-c-cardhead px-6 pt-3.5 pb-4.5">
          <button type="button" onClick={onClose} className="cursor-pointer text-sm font-medium text-c-ink2 hover:text-c-ink">
            Cancel
          </button>
          {error && <span className="text-sm text-c-warn">{error}</span>}
          <Button primary onClick={() => void discard()} loading={discarding}>
            Discard
          </Button>
        </div>
      </div>
    </div>
  );
}
