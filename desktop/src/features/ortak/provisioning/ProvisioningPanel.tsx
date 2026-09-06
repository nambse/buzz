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
import type { Employee } from "../types";
import { provisioningSteps } from "./types";
import { useProvisioning } from "./useProvisioning";

export function ProvisioningPanel({
  client,
  employee,
  onClose,
}: {
  client: OrtakClient;
  employee: Pick<Employee, "employee_id" | "name">;
  onClose: () => void;
}) {
  const [cursor, setCursor] = useState<string | undefined>();
  const [operationId, setOperationId] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const stepButtons = useRef(new Map<string, HTMLButtonElement>());
  const focusOperation = useRef<string | null>(null);
  const { page, detail, error, retrying } = useProvisioning(
    client,
    employee.employee_id,
    cursor,
    operationId,
    refresh,
  );
  useEffect(() => {
    if (page && operationId === null && focusOperation.current) {
      stepButtons.current.get(focusOperation.current)?.focus();
      focusOperation.current = null;
    }
  }, [page, operationId]);
  return (
    <section
      aria-label={`Provisioning for ${employee.name ?? employee.employee_id}`}
      className="flex flex-col gap-4"
    >
      <Card>
        <CardHeader>
          <CardTitle>
            {employee.name ?? employee.employee_id} provisioning
          </CardTitle>
          <CardDescription>
            Last saved progress. These records do not confirm that a runner is
            still connected or that the employee is healthy now.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">
            Read-only. Your deployment operator can continue or recover a
            provisioning operation.
          </p>
          {error ? (
            <Alert variant="destructive">
              <AlertTitle>Provisioning records unavailable</AlertTitle>
              <AlertDescription>
                {error}{" "}
                {retrying
                  ? "Retrying this read."
                  : "Use Refresh progress to try again."}
              </AlertDescription>
            </Alert>
          ) : !page ? (
            <Skeleton className="h-12 w-full" />
          ) : null}
          {page?.operations.length === 0 ? (
            <Alert role="status">
              <AlertDescription>
                No provisioning operations have been recorded for this employee.
              </AlertDescription>
            </Alert>
          ) : null}
          <ul
            aria-label="Provisioning operations"
            className="flex flex-col gap-2"
          >
            {page?.operations.map((operation) => (
              <li
                key={operation.operation_id}
                className="flex flex-wrap items-center justify-between gap-3"
              >
                <div className="flex flex-col gap-1">
                  <span className="text-sm">
                    {operation.dry_run
                      ? "Dry run"
                      : operation.mode === "adopt"
                        ? "Adopt prepared employee"
                        : operation.mode === "update"
                          ? "Update employee"
                          : "Create employee"}
                  </span>
                  <Badge
                    variant={
                      operation.status === "failed"
                        ? "destructive"
                        : "secondary"
                    }
                  >
                    {operation.status}
                  </Badge>
                  <time
                    className="text-xs text-muted-foreground"
                    dateTime={operation.updated_at}
                  >
                    Saved {new Date(operation.updated_at).toLocaleString()}
                  </time>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  aria-label={`View provisioning steps for operation ${operation.operation_id}`}
                  ref={(node) => {
                    if (node)
                      stepButtons.current.set(operation.operation_id, node);
                    else stepButtons.current.delete(operation.operation_id);
                  }}
                  aria-pressed={operationId === operation.operation_id}
                  onClick={() => {
                    setOperationId(operation.operation_id);
                  }}
                >
                  View steps
                </Button>
              </li>
            ))}
          </ul>
        </CardContent>
        <CardFooter className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setRefresh((value) => value + 1)}
          >
            Refresh progress
          </Button>
          {cursor ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setOperationId(null);
                setCursor(undefined);
              }}
            >
              Newest operations
            </Button>
          ) : null}
          {page?.has_more && page.next_cursor ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setOperationId(null);
                setCursor(page.next_cursor ?? undefined);
              }}
            >
              Older operations
            </Button>
          ) : null}
          <Button variant="ghost" size="sm" onClick={onClose}>
            Close provisioning
          </Button>
        </CardFooter>
      </Card>
      {detail ? (
        <Card>
          <CardHeader>
            <CardTitle>Recorded provisioning steps</CardTitle>
            <CardDescription>
              Operation {detail.operation.operation_id}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            {detail.runtime_probe ? (
              <Alert
                role="status"
                variant={
                  detail.runtime_probe.state === "failed"
                    ? "destructive"
                    : "default"
                }
              >
                <AlertTitle>
                  Runtime connection check · Attempt{" "}
                  {detail.runtime_probe.generation}
                </AlertTitle>
                <AlertDescription>
                  {detail.runtime_probe.state === "running"
                    ? "A connection check or its cleanup is pending. An interrupted check keeps its identity; recovery must confirm that process has stopped before starting another."
                    : detail.runtime_probe.state === "succeeded"
                      ? "The recorded connection check completed and its process was stopped. Activation still requires fresh current health checks."
                      : "The connection check did not establish readiness. Its process was stopped. Use the available command recovery action to try again."}
                </AlertDescription>
              </Alert>
            ) : null}
            {detail.operation.dry_run ? (
              <Alert role="status">
                <AlertDescription>
                  This is a dry run. It did not publish a profile or activate an
                  employee revision.
                </AlertDescription>
              </Alert>
            ) : null}
            {detail.operation.mode === "adopt" ? (
              <Alert role="status">
                <AlertDescription>
                  Existing resources are retained when this Adopt operation is
                  compensated.
                </AlertDescription>
              </Alert>
            ) : null}
            {detail.operation.status === "failed" ? (
              <Alert variant="destructive">
                <AlertTitle>Provisioning step failed</AlertTitle>
                <AlertDescription>
                  {detail.operation.current_step
                    ? provisioningSteps[detail.operation.current_step]
                    : "An operation step"}{" "}
                  did not finish successfully. The saved state remains available
                  to your deployment operator.
                </AlertDescription>
              </Alert>
            ) : null}
            {detail.operation.result_revision_id ? (
              <p className="text-sm">
                Committed revision: {detail.operation.result_revision_id}
              </p>
            ) : null}
            <ol
              aria-label="Recorded provisioning steps"
              className="flex flex-col gap-3"
            >
              {detail.steps.map((step) => (
                <li
                  key={step.name}
                  className="flex flex-wrap items-center gap-2"
                >
                  <span className="text-sm font-medium">
                    {provisioningSteps[step.name]}
                  </span>
                  <Badge
                    variant={
                      step.state === "failed" ? "destructive" : "secondary"
                    }
                  >
                    {step.state}
                  </Badge>
                  <span className="text-xs text-muted-foreground">
                    Attempts: {step.attempt_count}
                  </span>
                  {step.adopted_existing ? (
                    <span className="text-xs text-muted-foreground">
                      Existing resource retained
                    </span>
                  ) : null}
                </li>
              ))}
            </ol>
          </CardContent>
          <CardFooter>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                focusOperation.current = operationId;
                setOperationId(null);
              }}
            >
              Close steps
            </Button>
          </CardFooter>
        </Card>
      ) : null}
    </section>
  );
}
