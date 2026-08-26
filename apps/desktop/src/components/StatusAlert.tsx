import type { ReactNode } from "react";
import { Check, CircleAlert, TriangleAlert } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

type Props = {
  children: ReactNode;
  className?: string;
  role?: string;
};

export function ErrorAlert({ children, className, role }: Props) {
  return (
    <Alert variant="destructive" className={cn("error", className)} role={role ?? "alert"}>
      <CircleAlert />
      <AlertDescription>{children}</AlertDescription>
    </Alert>
  );
}

export function WarnAlert({ children, className, role }: Props) {
  return (
    <Alert className={cn("warn border-warn/30 bg-warn/10", className)} role={role ?? "alert"}>
      <TriangleAlert />
      <AlertDescription className="text-warn">{children}</AlertDescription>
    </Alert>
  );
}

export function OkAlert({ children, className }: Props) {
  return (
    <Alert className={cn("ok border-ok/30 bg-ok/10", className)}>
      <Check />
      <AlertDescription className="text-ok">{children}</AlertDescription>
    </Alert>
  );
}
