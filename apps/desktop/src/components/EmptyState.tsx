type Props = {
  title: string;
  description: string;
  actionLabel: string;
  onAction: () => void;
};

/** Friendly empty-state card with a single guided action. */
export function EmptyState({ title, description, actionLabel, onAction }: Props) {
  return (
    <div className="empty-state">
      <h3 className="empty-state-title">{title}</h3>
      <p className="empty-state-desc">{description}</p>
      <button type="button" onClick={onAction}>
        {actionLabel}
      </button>
    </div>
  );
}