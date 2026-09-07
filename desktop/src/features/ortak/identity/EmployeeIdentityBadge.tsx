import { Badge } from "@/shared/ui/badge";
import { useOfficeEmployee } from "./EmployeeDirectoryProvider";
import { employeeStateLabel } from "./directory";

/** Identity and recorded work do not imply legacy agent ownership or presence. */
export function EmployeeIdentityBadge({
  pubkey,
  showState = false,
}: {
  pubkey?: string | null;
  showState?: boolean;
}) {
  const value = useOfficeEmployee(pubkey);
  if (!value) return null;
  const state = employeeStateLabel(value);
  const role = value.employee.title ? ` · ${value.employee.title}` : "";
  return (
    <Badge
      variant="secondary"
      data-testid="ortak-employee-identity"
      title={`${value.employee.name ?? value.employee.employee_id}${role}. ${state}. Runtime health is shown in Employees.`}
      aria-label={`Employee ${value.employee.name ?? value.employee.employee_id}${role}. ${state}`}
    >
      {showState ? state : "Employee"}
    </Badge>
  );
}
