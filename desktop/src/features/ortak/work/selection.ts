export interface WorkSelection {
  project: string;
  item: string;
}

/** Navigation selects opaque IDs only; fresh signed reads still authorize every record. */
export function workSelection(
  project: unknown,
  item: unknown,
): WorkSelection | undefined {
  const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
  return typeof project === "string" &&
    typeof item === "string" &&
    uuid.test(project) &&
    uuid.test(item)
    ? { project, item }
    : undefined;
}
