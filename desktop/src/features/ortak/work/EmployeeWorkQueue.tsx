import { useEffect, useRef, useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import type { OrtakClient } from "../client";
import type { Employee } from "../types";
import { stateLabel } from "./operations";
import { useEmployeeWork } from "./useEmployeeWork";

export function EmployeeWorkQueue({
  client,
  employee,
  onClose,
}: {
  client: OrtakClient;
  employee: Employee;
  onClose: () => void;
}) {
  const [cursor, setCursor] = useState<string | undefined>();
  const [refresh, setRefresh] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    heading.current?.focus();
  }, []);
  const { page, error } = useEmployeeWork(
    client,
    employee.employee_id,
    cursor,
    refresh,
  );
  return (
    <section
      aria-label="Employee assigned work"
      className="flex flex-col gap-4 rounded-xl border bg-card p-5"
    >
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex flex-col gap-2">
          <h2
            ref={heading}
            tabIndex={-1}
            className="break-words text-lg font-semibold outline-none"
          >
            {employee.name ?? "Employee"}’s assigned work
          </h2>
          <p className="text-sm text-muted-foreground">
            Read-only manual assignments. These do not start or confirm employee
            execution.
          </p>
          {employee.status !== "active" ? (
            <p className="text-xs text-muted-foreground">
              Saved employee status: {employee.status}. Outstanding assignments
              remain visible while inactive.
            </p>
          ) : null}
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setCursor(undefined);
              setRefresh((value) => value + 1);
            }}
          >
            Refresh assigned work
          </Button>
          <Button size="sm" variant="ghost" onClick={onClose}>
            Close assigned work
          </Button>
        </div>
      </header>
      {error ? (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {!page && !error ? (
        <div role="status" className="flex flex-col gap-2">
          <span className="text-sm text-muted-foreground">
            Loading assigned work…
          </span>
          <Skeleton className="h-24 w-full" />
        </div>
      ) : null}
      {page?.work_items.length === 0 ? (
        <p role="status" className="text-sm text-muted-foreground">
          No visible outstanding assignments in this page.
        </p>
      ) : null}
      {page?.work_items.length ? (
        <ul
          aria-label="Outstanding manual assignments"
          className="grid gap-3 md:grid-cols-2"
        >
          {page.work_items.map((item) => (
            <li
              key={item.id}
              className="flex min-w-0 flex-col gap-2 rounded-lg border p-4"
            >
              <h3 className="break-words text-sm font-semibold">
                {item.title}
              </h3>
              <div className="flex flex-wrap gap-2">
                <Badge variant="secondary">{stateLabel(item.state)}</Badge>
                <Badge variant="outline">{item.priority}</Badge>
              </div>
              <p className="text-sm">Assignment role: {item.assignment_role}</p>
              <p className="text-xs text-muted-foreground">
                Saved manual state · Version {item.version}
              </p>
            </li>
          ))}
        </ul>
      ) : null}
      <div className="flex flex-wrap gap-2">
        {cursor ? (
          <Button
            size="sm"
            variant="outline"
            onClick={() => setCursor(undefined)}
          >
            First assignments
          </Button>
        ) : null}
        {page?.next_cursor ? (
          <Button
            size="sm"
            variant="outline"
            onClick={() => setCursor(page.next_cursor ?? undefined)}
          >
            More assignments
          </Button>
        ) : null}
      </div>
    </section>
  );
}
