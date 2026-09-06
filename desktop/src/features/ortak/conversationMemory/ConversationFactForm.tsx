import { useEffect, useId, useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { Field } from "../work/fields";
import type { ConversationDraft, ConversationPreview } from "./types";
import type { useConversationMutation } from "./useConversationMutation";

/** The source is only a reference; edited text always starts empty. */
export function ConversationFactForm({
  preview,
  projectName,
  employee,
  employeeName,
  message,
  mutation,
  disabled,
  approve,
}: {
  preview: ConversationPreview;
  projectName: string;
  employee: string;
  employeeName: string;
  message: string;
  mutation: ReturnType<typeof useConversationMutation>;
  disabled: boolean;
  approve: (draft: ConversationDraft) => void;
}) {
  const [content, setContent] = useState("");
  const [expiry, setExpiry] = useState("");
  const [reviewedPreview, setReviewed] = useState<ConversationPreview | null>(
    null,
  );
  const reviewed = reviewedPreview === preview;
  const [error, setError] = useState<string | null>(null);
  const reviewId = useId();
  const allowedUntil = Math.min(
    new Date(preview.max_expires_at).getTime(),
    preview.valid_before ? new Date(preview.valid_before).getTime() : Infinity,
  );
  const [expired, setExpired] = useState(allowedUntil <= Date.now());
  useEffect(() => {
    setExpired(allowedUntil <= Date.now());
    if (!Number.isFinite(allowedUntil) || allowedUntil <= Date.now()) return;
    let timer: ReturnType<typeof setTimeout>;
    const check = () => {
      const delay = allowedUntil - Date.now();
      if (delay <= 0) setExpired(true);
      else timer = setTimeout(check, Math.min(delay, 2_147_000_000));
    };
    check();
    return () => clearTimeout(timer);
  }, [allowedUntil]);
  const blocked =
    disabled || expired || mutation.busy || Boolean(mutation.pending);
  return (
    <form
      aria-label="Approve conversation fact"
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (blocked || allowedUntil <= Date.now()) return;
        const ends = new Date(expiry);
        if (
          !reviewed ||
          !content.trim() ||
          new TextEncoder().encode(content).length > 4096 ||
          !Number.isFinite(ends.getTime()) ||
          ends.getTime() <= Date.now() ||
          ends.getTime() > allowedUntil
        ) {
          setError(
            "Review the audience and edited text, keep the text within 4 KiB, and choose a future expiry within the displayed limit.",
          );
          return;
        }
        setError(null);
        approve({
          employee_id: employee,
          source_message_id: message,
          audience: { kind: preview.audience.kind },
          expected_audience_hash: preview.audience_hash,
          content,
          expires_at: ends.toISOString(),
          reviewed: true,
        });
      }}
    >
      <Alert role="status">
        <AlertDescription className="flex flex-col gap-2">
          <p>
            <strong>Audience to review:</strong> {employeeName} in {projectName}{" "}
            ·{" "}
            {preview.audience.kind === "thread"
              ? "only this canonical thread"
              : "this entire Office channel"}
            .
          </p>
          <p>
            Approval is limited to this audience. The server rechecks the source
            and permissions when you save.
          </p>
          <details>
            <summary className="cursor-pointer">
              Canonical audience details
            </summary>
            <p className="break-all">Channel {preview.audience.channel_id}</p>
            {preview.audience.thread_root_event_id ? (
              <p className="break-all">
                Thread root {preview.audience.thread_root_event_id}
              </p>
            ) : null}
            <p className="break-all">Audience hash {preview.audience_hash}</p>
          </details>
        </AlertDescription>
      </Alert>
      {error ? (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {expired ? (
        <p role="status" className="text-sm">
          This audience observation has expired. Refresh the audience before
          approval.
        </p>
      ) : null}
      <fieldset disabled={blocked} className="flex flex-col gap-3">
        <legend className="sr-only">Edited fact and review</legend>
        <Field label="Edited fact text">
          {(id) => (
            <Textarea
              id={id}
              name="content"
              required
              value={content}
              onChange={(event) => {
                setContent(event.target.value);
                setReviewed(null);
              }}
            />
          )}
        </Field>
        <Field label="Use until">
          {(id) => (
            <Input
              id={id}
              name="expiry"
              type="datetime-local"
              required
              value={expiry}
              onChange={(event) => {
                setExpiry(event.target.value);
                setReviewed(null);
              }}
            />
          )}
        </Field>
        <p className="text-xs text-muted-foreground">
          Latest permitted expiry:{" "}
          <time dateTime={new Date(allowedUntil).toISOString()}>
            {new Date(allowedUntil).toLocaleString()}
          </time>
          . Stored approval history is retained after use ends.
        </p>
        <div className="flex items-start gap-2">
          <Checkbox
            id={reviewId}
            checked={reviewed}
            onCheckedChange={(value) =>
              setReviewed(value === true ? preview : null)
            }
          />
          <label htmlFor={reviewId} className="text-sm">
            I reviewed this edited text and its displayed conversation audience.
          </label>
        </div>
        <Button type="submit" disabled={blocked || !reviewed}>
          Approve conversation fact
        </Button>
      </fieldset>
    </form>
  );
}
