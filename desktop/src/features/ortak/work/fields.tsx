import { useId, type ReactNode, type SelectHTMLAttributes } from "react";

// The retained component set has no Field or Select. Keep native form semantics
// and match the installed Input's focus and semantic color tokens.
export function Field({
  label,
  children,
}: {
  label: string;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="flex flex-col gap-2">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      {children(id)}
    </div>
  );
}
export function Select(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className="h-9 w-full rounded-lg border border-input/40 bg-background px-3 text-sm focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
    />
  );
}
export type SubmitWork = (
  path: string,
  label: string,
  values: Record<string, unknown>,
) => void;
