import { Button } from "@/components/ui/button";
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

type Props = {
  title: string;
  description: string;
  actionLabel: string;
  onAction: () => void;
};

/** Friendly empty-state card with a single guided action. */
export function EmptyState({ title, description, actionLabel, onAction }: Props) {
  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
        <div>
          <Button type="button" size="sm" onClick={onAction}>
            {actionLabel}
          </Button>
        </div>
      </CardHeader>
    </Card>
  );
}
