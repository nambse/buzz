import type { NativeDm } from "./types";
import { useConfidentialDm } from "./useConfidentialDm";

/** Root mounts this instead of the ordinary composer/timeline for an explicitly
 * selected encrypted pair. The component itself never enables that selection. */
export function ConfidentialDm({
  selected,
  employeeName,
  native,
}: {
  selected: { channelId: string; human: string; relay: string } | null;
  employeeName: string;
  native?: NativeDm;
}) {
  const dm = useConfidentialDm(selected, native);
  const pending = dm.view?.pending;
  return (
    <section
      aria-label="Encrypted conversation"
      className="flex h-full flex-col gap-3 p-4"
    >
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-base font-semibold">
          Private conversation with {employeeName}
        </h2>
        <button
          type="button"
          onClick={dm.refresh}
          disabled={dm.busy}
          className="text-sm underline"
        >
          Refresh messages
        </button>
      </div>
      <p className="text-xs text-muted-foreground">
        Messages and saved drafts are encrypted. This view locks when you leave
        it.
      </p>
      {dm.error && (
        <p role="alert" className="text-sm">
          {dm.error}
        </p>
      )}
      {dm.note && (
        <p role="status" className="text-sm">
          {dm.note}
        </p>
      )}
      {dm.sealing && (
        <p role="status" className="text-xs">
          Saving encrypted draft…
        </p>
      )}
      {dm.view && (
        <>
          <ol
            aria-label="Decrypted messages in this pair"
            className="min-h-0 flex-1 space-y-3 overflow-auto"
          >
            {dm.view.messages.map((message) => (
              <li key={message.rumor_id} className="space-y-1">
                <p className="text-xs font-medium">
                  {message.sender === selected?.human ? "You" : employeeName}
                </p>
                <p className="whitespace-pre-wrap break-words text-message">
                  {message.text}
                </p>
              </li>
            ))}
          </ol>
          {dm.view.limited && (
            <p className="text-xs">
              Only the latest bounded recipient snapshot is shown.
            </p>
          )}
          {dm.view.withheld_count > 0 && (
            <p className="text-xs">
              Some encrypted entries could not be shown for this pair.
            </p>
          )}
          {dm.view.retired.length > 0 && (
            <div className="space-y-1 text-xs">
              <p>
                Retained sends are kept without retry. Delivery may have
                occurred.
              </p>
              <ol aria-label="Retained encrypted sends">
                {dm.view.retired.map((receipt) => (
                  <li key={receipt.operation_id}>
                    Send {receipt.operation_id}: retained without retry;
                    acknowledged copies{" "}
                    {receipt.acknowledged.filter(Boolean).length}/2.
                  </li>
                ))}
              </ol>
            </div>
          )}
          {pending ? (
            <div className="space-y-2">
              <p className="text-sm">
                An encrypted send still needs confirmation. New sends are held.
              </p>
              {pending.scope === dm.view.scope ? (
                <button
                  type="button"
                  disabled={dm.busy}
                  onClick={() => void dm.retry()}
                  className="text-sm underline"
                >
                  Retry retained encrypted send
                </button>
              ) : (
                <p className="text-sm">
                  Its original authority changed. The frozen copies remain
                  retained for recovery.
                </p>
              )}
              <p className="text-sm">
                This send may already have been delivered. Keeping it stops
                retries without undoing delivery. The new draft starts empty.
              </p>
              <button
                type="button"
                disabled={dm.busy || dm.sealing}
                onClick={() => void dm.retire()}
                className="text-sm underline"
              >
                Keep old send and start new draft
              </button>
            </div>
          ) : (
            <form
              aria-label="Send encrypted message"
              onSubmit={(event) => {
                event.preventDefault();
                void dm.send();
              }}
              className="space-y-2"
            >
              <label htmlFor="ortak-confidential-composer" className="text-sm">
                Encrypted message
              </label>
              <textarea
                id="ortak-confidential-composer"
                value={dm.text}
                onChange={(event) => dm.edit(event.target.value)}
                disabled={dm.busy}
                autoComplete="off"
                autoCorrect="off"
                spellCheck={false}
                className="min-h-24 w-full rounded border bg-background p-2 text-message"
              />
              <button
                type="submit"
                disabled={dm.busy || dm.sealing || !dm.text.trim()}
                className="text-sm underline"
              >
                Send encrypted message
              </button>
            </form>
          )}
        </>
      )}
    </section>
  );
}
