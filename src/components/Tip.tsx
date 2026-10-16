import type { ReactNode } from "react";

/**
 * Help for a form field, shown while the pointer is over the field or the
 * field has focus. Put it last inside a `.field.has-tip`, and point the
 * input's aria-describedby at `id` so screen readers read it too.
 */
export function Tip({ id, children }: { id: string; children: ReactNode }) {
  return (
    <span id={id} className="tip" role="tooltip">
      {children}
    </span>
  );
}

/** The "?" after a label, telling people there is help to hover for. */
export function TipMark() {
  return (
    <span className="tip-mark" aria-hidden="true">
      ?
    </span>
  );
}
