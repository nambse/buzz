import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import {
  provisioningSteps,
  type ProvisioningDetail,
  type ProvisioningPage,
} from "./types";

/** One generation of persisted progress; reads never resume the operator runner. */
export function useProvisioning(
  client: OrtakClient,
  employeeId: string,
  cursor: string | undefined,
  operationId: string | null,
  refresh: number,
) {
  const key = JSON.stringify([employeeId, cursor, operationId, refresh]);
  type State = {
    client: OrtakClient;
    key: string;
    page: ProvisioningPage | null;
    detail: ProvisioningDetail | null;
    error: string | null;
    retrying: boolean;
  };
  const empty: State = {
    client,
    key,
    page: null,
    detail: null,
    error: null,
    retrying: false,
  };
  const [state, setState] = useState<State>(empty);
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setState({
      client,
      key,
      page: null,
      detail: null,
      error: null,
      retrying: false,
    });
    async function read() {
      // On every failure preserve the typed authorization outcome before abort
      // can replace it with a generic reader-cleanup exception.
      try {
        const page = await client.provisioning(
          employeeId,
          controller.signal,
          cursor,
        );
        if (controller.signal.aborted) return;
        if (
          page.employee_id !== employeeId ||
          page.read_only !== true ||
          page.operations.length > 25 ||
          page.operations.some(
            (operation) => operation.employee_id !== employeeId,
          )
        )
          throw new Error("Provisioning records did not match this employee.");
        const detail = operationId
          ? await client.provisioningOperation(
              employeeId,
              operationId,
              controller.signal,
            )
          : null;
        if (controller.signal.aborted) return;
        if (
          detail &&
          (detail.read_only !== true ||
            detail.operation.employee_id !== employeeId ||
            detail.operation.operation_id !== operationId ||
            detail.steps.length !== Object.keys(provisioningSteps).length ||
            detail.steps.some(
              (step, index) =>
                step.name !== Object.keys(provisioningSteps)[index],
            ))
        )
          throw new Error("Provisioning steps did not match this operation.");
        setState({ client, key, page, detail, error: null, retrying: false });
        failures = 0;
        timer = setTimeout(() => void read(), 5000);
      } catch (cause) {
        if (controller.signal.aborted) return;
        const revoked =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        failures++;
        const retrying = !revoked && failures < 5;
        setState({
          client,
          key,
          page: null,
          detail: null,
          error:
            cause instanceof OrtakApiError && cause.status >= 500
              ? "Provisioning records could not be read."
              : cause instanceof Error
                ? cause.message
                : "Provisioning records could not be read.",
          retrying,
        });
        if (retrying)
          timer = setTimeout(
            () => void read(),
            Math.min(3000 * 2 ** (failures - 1), 30_000),
          );
      }
    }
    void read();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, employeeId, cursor, operationId, key]);
  return state.client === client && state.key === key ? state : empty;
}
