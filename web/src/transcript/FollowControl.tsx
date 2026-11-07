import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";

/** Omits a zero unseen count while preserving the action for a reader above the end. */
export function FollowControl({
  unseen,
  onResume,
}: {
  unseen: number;
  onResume: () => void;
}) {
  const t = useT();
  return (
    <div className="follow-controls">
      <Button variant="secondary" onClick={onResume}>
        {unseen > 0 ? t("follow.jumpNew", { count: unseen }) : t("follow.jump")}
      </Button>
    </div>
  );
}
