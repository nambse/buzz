import { useEffect, useRef, useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { OrtakApiError, type OrtakClient } from "../client";
import { Field } from "./fields";
import type { ReviewedRecall as Recall } from "./memoryTypes";

export function ReviewedRecall({
  client,
  project,
  employee,
  stamp,
  revoke,
}: {
  client: OrtakClient;
  project: string;
  employee: string;
  stamp: number;
  revoke: () => void;
}) {
  const [result, setResult] = useState<Recall | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => {
    void stamp;
    controller.current?.abort();
    setResult(null);
    setError(null);
    setBusy(false);
    return () => controller.current?.abort();
  }, [stamp]);
  return (
    <section
      aria-label="Reviewed recall preview"
      className="flex flex-col gap-3"
    >
      <form
        aria-label="Search reviewed context"
        className="flex flex-col gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          controller.current?.abort();
          const attempt = new AbortController();
          controller.current = attempt;
          setResult(null);
          setError(null);
          setBusy(true);
          const query = String(
            new FormData(event.currentTarget).get("query") ?? "",
          );
          try {
            const page = await client.recallReviewedMemory(
              project,
              employee,
              query,
              attempt.signal,
            );
            if (!attempt.signal.aborted) setResult(page);
          } catch (cause) {
            if (attempt.signal.aborted) return;
            setError(
              cause instanceof Error
                ? cause.message
                : "Recall is unavailable. Search again to retry.",
            );
            if (
              cause instanceof OrtakApiError &&
              [401, 403, 404].includes(cause.status)
            )
              revoke();
          } finally {
            if (!attempt.signal.aborted) setBusy(false);
          }
        }}
      >
        <Field label="Search approved facts">
          {(id) => <Input id={id} name="query" required maxLength={1024} />}
        </Field>
        <Button
          type="submit"
          variant="outline"
          disabled={busy}
          className="self-start"
        >
          {busy ? "Searching…" : "Preview recall"}
        </Button>
      </form>
      <p className="text-xs text-muted-foreground">
        Up to eight active facts and 8 KiB for this audience. This preview
        clears with the next access check. It is not sent to employee runs.
      </p>
      {error ? (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {result ? (
        <div role="status" className="flex flex-col gap-2 text-sm">
          {!result.facts.length ? (
            <p>No active approved facts matched.</p>
          ) : (
            result.facts.map((fact) => (
              <p key={fact.id} className="whitespace-pre-wrap break-words">
                {fact.content}
              </p>
            ))
          )}
          {result.truncated ? (
            <p>Some matches exceeded the preview limit.</p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
