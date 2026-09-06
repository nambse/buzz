import { useEffect, useRef, useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";
import { Skeleton } from "@/shared/ui/skeleton";
import type { OrtakClient } from "../client";
import { useManagement } from "./useManagement";
import type { ManagementCommand } from "./managementTypes";
import { ProvisioningPanel } from "./ProvisioningPanel";

export function ManagementPanel({ client }: { client: OrtakClient }) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const restore = useRef(false);
  useEffect(() => {
    if (!open && restore.current) {
      restore.current = false;
      trigger.current?.focus();
    }
  }, [open]);
  return (
    <section aria-label="Prepared employee management">
      <Button
        ref={trigger}
        variant="outline"
        aria-expanded={open}
        onClick={() => setOpen(true)}
      >
        Manage prepared employees
      </Button>
      {open ? (
        <ManagementContents
          client={client}
          onClose={() => {
            restore.current = true;
            setOpen(false);
          }}
        />
      ) : null}
    </section>
  );
}

function ManagementContents({
  client,
  onClose,
}: {
  client: OrtakClient;
  onClose: () => void;
}) {
  const management = useManagement(client);
  const [showSteps, setShowSteps] = useState(false);
  const draftTitle = useRef<HTMLDivElement>(null);
  const draftId = management.draft?.draft_id;
  useEffect(() => {
    if (draftId) draftTitle.current?.focus();
  }, [draftId]);
  const disabled =
    management.busy || management.retryable || !management.catalog;
  const page = management.page;
  function recover(command: ManagementCommand, action: "retry" | "compensate") {
    if (!page) return;
    void management.command(page.employee_id, {
      idempotency_key: crypto.randomUUID(),
      action:
        action === "retry" && command.action === "reenable"
          ? "reenable"
          : action,
      operation_id: command.operation_id,
      draft_id: null,
      expected_revision_id: page.expected_revision_id,
      expected_lifecycle_epoch: page.expected_lifecycle_epoch,
    });
  }
  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Prepared employees</CardTitle>
          <CardDescription>
            Choose a configuration prepared by your deployment operator. Model
            and thinking choices use exact registered Hermes profiles and the
            selected credential reference.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <p className="text-sm text-muted-foreground">
            Saving a draft does not start a runtime or confirm health.
            Activation checks the real runtime, memory, Office membership and
            signer before saving an active revision. Prepared resources are
            retained during compensation.
          </p>
          {management.error ? (
            <Alert variant="destructive">
              <AlertTitle>Employee management unavailable</AlertTitle>
              <AlertDescription>{management.error}</AlertDescription>
            </Alert>
          ) : null}
          {!management.catalog && !management.error ? (
            <Skeleton className="h-12 w-full" />
          ) : null}
          {management.catalog?.choices.length === 0 ? (
            <Alert>
              <AlertDescription>
                No prepared configurations are currently available to your
                account.
              </AlertDescription>
            </Alert>
          ) : null}
          {management.catalog?.employees?.length ? (
            <ul
              aria-label="Existing employees"
              className="flex flex-wrap gap-2"
            >
              {management.catalog.employees.map((employee) => (
                <li key={employee.employee_id}>
                  <Button
                    variant="outline"
                    disabled={management.busy}
                    onClick={() => {
                      setShowSteps(false);
                      management.selectEmployee(employee.employee_id);
                    }}
                  >
                    Manage {employee.employee_id} · {employee.status}
                  </Button>
                </li>
              ))}
            </ul>
          ) : null}
          <ul
            aria-label="Prepared configurations"
            className="flex flex-col gap-3"
          >
            {management.catalog?.choices.map((choice) => (
              <li
                key={choice.catalog_id}
                className="flex flex-wrap items-center justify-between gap-3"
              >
                <div>
                  <p className="text-sm">
                    {choice.label} · {choice.employee_id}
                  </p>
                  <p className="text-sm text-muted-foreground">
                    {choice.model} · Thinking:{" "}
                    {choice.thinking ?? "Profile default"}
                  </p>
                  {!choice.can_save_draft ? (
                    <p className="text-sm text-muted-foreground">
                      Re-enable requires the lifecycle workflow, which is not
                      available yet.
                    </p>
                  ) : null}
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={disabled || !choice.can_save_draft}
                    aria-label={`Save draft for ${choice.label}`}
                    onClick={() => {
                      setShowSteps(false);
                      void management.saveDraft(choice.employee_id, {
                        draft_id: crypto.randomUUID(),
                        catalog_id: choice.catalog_id,
                        expected_revision_id: choice.expected_revision_id,
                        expected_lifecycle_epoch:
                          choice.expected_lifecycle_epoch,
                      });
                    }}
                  >
                    Save draft
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={management.busy}
                    aria-label={`View commands for ${choice.employee_id}`}
                    onClick={() => {
                      setShowSteps(false);
                      management.selectEmployee(choice.employee_id);
                    }}
                  >
                    View commands
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        </CardContent>
        <CardFooter className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={management.refresh}>
            Refresh management
          </Button>
          {management.retryable ? (
            <Button
              disabled={management.busy}
              onClick={management.retryRequest}
            >
              Retry same request
            </Button>
          ) : null}
          <Button variant="ghost" onClick={onClose}>
            Close management
          </Button>
        </CardFooter>
      </Card>
      {page?.lifecycle_supported && page.lifecycle ? (
        <Card>
          <CardHeader>
            <CardTitle>Availability for {page.employee_id}</CardTitle>
            <CardDescription>
              Last saved status: {page.status}. Lifecycle{" "}
              {page.expected_lifecycle_epoch}.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <p className="text-sm">
              Disable blocks new work and permanently invalidates earlier queued
              work and pending output. Prepared resources and history are
              retained. Running processes stop when the worker confirms
              cancellation.
            </p>
            {page.status === "disabled" ? (
              <p className="text-sm">
                To re-enable, save a prepared configuration above and run its
                fresh activation checks. Earlier work will stay cancelled or
                ineligible.
              </p>
            ) : null}
            <p className="text-sm">
              Earlier runs still active: {page.lifecycle.old_active_runs}.
              Pending stops: {page.lifecycle.pending_stops}. Failed stops:{" "}
              {page.lifecycle.failed_stops}.
            </p>
          </CardContent>
          {page.lifecycle.can_disable ? (
            <CardFooter>
              <Button
                variant="destructive"
                disabled={disabled}
                onClick={() => {
                  void management.command(page.employee_id, {
                    idempotency_key: crypto.randomUUID(),
                    action: "disable",
                    draft_id: null,
                    operation_id: null,
                    expected_revision_id: page.expected_revision_id,
                    expected_lifecycle_epoch: page.expected_lifecycle_epoch,
                  });
                }}
              >
                Disable {page.employee_id}
              </Button>
            </CardFooter>
          ) : null}
        </Card>
      ) : null}
      {management.draft ? (
        <Card>
          <CardHeader>
            <CardTitle ref={draftTitle} tabIndex={-1}>
              Saved draft for {management.draft.employee_id}
            </CardTitle>
            <CardDescription>
              {management.draft.model} · Thinking:{" "}
              {management.draft.thinking ?? "Profile default"}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-sm">
              {management.draft.action === "reenable"
                ? "The employee stays disabled until every fresh health check passes and a new revision is saved. Earlier work cannot resume."
                : management.draft.action === "update"
                  ? "The current revision stays active until the new configuration passes every activation check."
                  : "This adopts the selected prepared resources and publishes the employee's Office profile after verification."}
            </p>
          </CardContent>
          <CardFooter>
            <Button
              disabled={disabled}
              onClick={() => {
                const draft = management.draft;
                if (draft)
                  void management.command(draft.employee_id, {
                    idempotency_key: crypto.randomUUID(),
                    action: draft.action,
                    draft_id: draft.draft_id,
                    operation_id: null,
                    expected_revision_id: draft.expected_revision_id,
                    expected_lifecycle_epoch: draft.expected_lifecycle_epoch,
                  });
              }}
            >
              {management.draft.action === "reenable"
                ? "Check and re-enable"
                : "Check and activate"}
            </Button>
          </CardFooter>
        </Card>
      ) : null}
      {page ? (
        <Card>
          <CardHeader>
            <CardTitle>Commands for {page.employee_id}</CardTitle>
            <CardDescription>
              Last saved state, refreshed every five seconds while this panel is
              open. A queued command has not activated the employee.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            {page.commands.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No commands have been recorded for this employee.
              </p>
            ) : null}
            <ul aria-label="Employee commands" className="flex flex-col gap-3">
              {page.commands.map((command) => (
                <li key={command.command_id} className="flex flex-col gap-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm">{command.action}</span>
                    <Badge
                      variant={
                        command.status === "failed" ||
                        command.status === "blocked"
                          ? "destructive"
                          : "secondary"
                      }
                    >
                      {command.status}
                    </Badge>
                    <span className="text-xs text-muted-foreground">
                      Attempt {command.attempts} of 3
                    </span>
                  </div>
                  <time
                    dateTime={command.updated_at}
                    className="text-xs text-muted-foreground"
                  >
                    Saved {new Date(command.updated_at).toLocaleString()}
                  </time>
                  {command.error_code ? (
                    <p className="text-sm text-muted-foreground">
                      {command.status === "blocked"
                        ? "Current authority no longer allows this command. Refresh after an authorized operator restores access."
                        : command.error_code === "command_attempts_exhausted"
                          ? "Automatic recovery attempts are exhausted. Review the retained operation before choosing an available recovery action."
                          : "The attempt did not complete. Its durable operation and prepared resources are retained."}
                    </p>
                  ) : null}
                  {command.runtime_probe ? (
                    <p role="status" className="text-sm text-muted-foreground">
                      Connection check {command.runtime_probe.generation}:{" "}
                      {command.runtime_probe.state === "running"
                        ? "execution or cleanup is pending; the same process must be recovered first."
                        : command.runtime_probe.state === "succeeded"
                          ? "completed and stopped; activation still requires current health."
                          : command.runtime_probe.generation >= 20
                            ? "the attempt limit was reached. Review a new prepared configuration before another activation attempt."
                            : "failed and stopped; the saved command recovery action can retry it."}
                    </p>
                  ) : null}
                  <div className="flex flex-wrap gap-2">
                    {command.can_retry ? (
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={disabled}
                        aria-label={`Retry operation ${command.operation_id}`}
                        onClick={() => recover(command, "retry")}
                      >
                        Retry operation
                      </Button>
                    ) : null}
                    {command.can_compensate ? (
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={disabled}
                        aria-label={`Retain prepared resources for ${command.operation_id}`}
                        onClick={() => recover(command, "compensate")}
                      >
                        Finish and retain resources
                      </Button>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          </CardContent>
          <CardFooter>
            {page.commands.some((command) => command.operation_id) ? (
              <Button variant="outline" onClick={() => setShowSteps(true)}>
                View recorded provisioning steps
              </Button>
            ) : null}
          </CardFooter>
        </Card>
      ) : null}
      {showSteps && page && management.catalog ? (
        <ProvisioningPanel
          client={client}
          employee={{ employee_id: page.employee_id, name: null }}
          onClose={() => setShowSteps(false)}
        />
      ) : null}
    </div>
  );
}
