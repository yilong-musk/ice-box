import { Button } from "@/components/ui/button";
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";

type Props = {
  title: string;
  description: string;
  actionLabel: string;
  onAction: () => void;
  className?: string;
  /** When false, render the copy and action without an extra card frame. */
  framed?: boolean;
};

/** Friendly empty-state card with a single guided action. */
export function EmptyState({
  title,
  description,
  actionLabel,
  onAction,
  className,
  framed = true,
}: Props) {
  const body = (
    <>
      <CardTitle>{title}</CardTitle>
      <CardDescription>{description}</CardDescription>
      <div>
        <Button type="button" size="sm" onClick={onAction}>
          {actionLabel}
        </Button>
      </div>
    </>
  );

  if (!framed) {
    return (
      <div className={cn("flex flex-col items-start gap-1", className)}>
        {body}
      </div>
    );
  }

  return (
    <Card size="sm" className={className}>
      <CardHeader>{body}</CardHeader>
    </Card>
  );
}
