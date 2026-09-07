import { useEffect, useId, useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { Field } from "../work/fields";
import type { EmployeeDraft, EmployeePreview } from "./types";

/** No source body enters this form. Review is bound to the exact returned preview. */
export function EmployeeMemoryForm({
  preview,
  employeeName,
  destinationName,
  disabled,
  approve,
}: {
  preview: EmployeePreview;
  employeeName: string;
  destinationName: string;
  disabled: boolean;
  approve: (draft: EmployeeDraft) => void;
}) {
  const [content, setContent] = useState("");
  const [expiry, setExpiry] = useState("");
  const [reviewedPreview, setReviewed] = useState<EmployeePreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reviewId = useId();
  const until = Math.min(
    Date.parse(preview.max_expires_at),
    preview.valid_before ? Date.parse(preview.valid_before) : Infinity,
  );
  const [expired, setExpired] = useState(until <= Date.now());
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const check = () => {
      const delay = until - Date.now();
      setExpired(delay <= 0);
      if (delay > 0) timer = setTimeout(check, Math.min(delay, 2_147_000_000));
    };
    check();
    return () => clearTimeout(timer);
  }, [until]);
  const blocked = disabled || expired;
  return (
    <form
      aria-label="Approve employee memory"
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (blocked || until <= Date.now()) return;
        const ends = Date.parse(expiry);
        const hasControl = Array.from(content).some((character) => {
          const code = character.codePointAt(0) ?? 0;
          return (
            (code < 32 && code !== 9 && code !== 10) ||
            (code >= 127 && code <= 159)
          );
        });
        if (
          reviewedPreview !== preview ||
          !content.trim() ||
          hasControl ||
          new TextEncoder().encode(content).length > 4096 ||
          !(Date.now() < ends && ends <= until)
        ) {
          setError(
            "Review the audience and edited text, use at most 4 KiB, and choose a future expiry within the displayed limit.",
          );
          return;
        }
        setError(null);
        approve({
          source_event_id: preview.source.event_id,
          source_event_created_at: preview.source.event_created_at,
          destination_channel_id: preview.audience.destination_channel_id,
          kind: preview.audience.kind,
          human_public_key: preview.audience.human_public_key,
          expected_audience_hash: preview.audience_hash,
          content,
          expires_at: new Date(ends).toISOString(),
          reviewed: true,
        });
      }}
    >
      <Alert role="status">
        <AlertDescription className="flex flex-col gap-2">
          <p>
            <strong>Audience to review:</strong> {employeeName} ·{" "}
            {destinationName} ·{" "}
            {preview.audience.kind === "relationship"
              ? "relationship with you"
              : "shared experience"}
            .
          </p>
          <p>
            This approval is limited to this destination channel. The server
            rechecks your source and both memberships when saving.
          </p>
          <details>
            <summary className="cursor-pointer">
              Verified destination details
            </summary>
            <p className="break-all">
              Channel {preview.audience.destination_channel_id}
            </p>
            <p className="break-all">Audience hash {preview.audience_hash}</p>
            {preview.audience.human_public_key ? (
              <p className="break-all">
                Relationship participant: you (
                {preview.audience.human_public_key})
              </p>
            ) : null}
          </details>
        </AlertDescription>
      </Alert>
      {error ? (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {expired ? (
        <p role="status">This preview has expired. Refresh before approving.</p>
      ) : null}
      <fieldset disabled={blocked} className="flex flex-col gap-3">
        <legend className="sr-only">
          Edited employee memory and sharing approval
        </legend>
        <Field label="Edited memory text">
          {(id) => (
            <Textarea
              id={id}
              name="content"
              maxLength={4096}
              required
              value={content}
              onChange={(event) => {
                setContent(event.target.value);
                setReviewed(null);
              }}
            />
          )}
        </Field>
        <Field label="Approval expires">
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
          Latest expiry:{" "}
          <time dateTime={new Date(until).toISOString()}>
            {new Date(until).toLocaleString()}
          </time>
          . Stopping or expiry retains the approval history.
        </p>
        <div className="flex items-start gap-2">
          <Checkbox
            id={reviewId}
            checked={reviewedPreview === preview}
            onCheckedChange={(checked) =>
              setReviewed(checked === true ? preview : null)
            }
          />
          <label htmlFor={reviewId} className="text-sm">
            I explicitly approve sharing this edited text with this Employee in
            the displayed destination
            {preview.audience.kind === "relationship"
              ? ", about their relationship with me"
              : ""}
            .
          </label>
        </div>
        <Button type="submit" disabled={blocked || reviewedPreview !== preview}>
          Approve employee memory
        </Button>
      </fieldset>
    </form>
  );
}
