import { useTranslation } from "react-i18next";
import type { RankedWord } from "@/api/hotwordPriority";

export default function HotwordPriorityDetails({ word, disabled, onReset }: {
  word: RankedWord; disabled: boolean; onReset: () => void;
}) {
  const { t } = useTranslation();
  const time = (at: number) => at ? new Date(at * 1000).toLocaleString() : t("hotwordPriority.never");
  const reason = (value: string) => ({
    usage: t("hotwordPriority.reasonUsage"), correction: t("hotwordPriority.reasonCorrection"),
    revision: t("hotwordPriority.reasonRevision"), decay: t("hotwordPriority.reasonDecay"),
    reset: t("hotwordPriority.reasonReset"), enabled: t("hotwordPriority.reasonEnabled"), disabled: t("hotwordPriority.reasonDisabled"),
  })[value] ?? value;
  return <section className="vocabulary-priority-details" aria-label={t("hotwordPriority.details", { word: word.text })}>
    <dl>
      <div><dt>{t("hotwordPriority.weights")}</dt><dd>{word.base_weight} → {word.effective_weight}</dd></div>
      <div><dt>{t("hotwordPriority.uses")}</dt><dd>{word.uses}</dd></div>
      <div><dt>{t("hotwordPriority.corrections")}</dt><dd>{word.corrections}</dd></div>
      <div><dt>{t("hotwordPriority.score")}</dt><dd>{word.score.toFixed(2)}</dd></div>
      <div><dt>{t("hotwordPriority.next")}</dt><dd>{word.next_threshold ?? t("hotwordPriority.maximum")}</dd></div>
      <div><dt>{t("hotwordPriority.lastUsed")}</dt><dd>{time(word.last_used)}</dd></div>
    </dl>
    <p className="vocabulary-muted">{t("hotwordPriority.scoringHint")}</p>
    <h3>{t("hotwordPriority.events")}</h3>
    {word.events.length ? <ol className="vocabulary-events">{word.events.map((event, index) => <li key={`${event.at}-${index}`}>
      <time dateTime={new Date(event.at * 1000).toISOString()}>{time(event.at)}</time>
      <span>{reason(event.reason)} · {event.before} → {event.after}</span>
    </li>)}</ol> : <p className="vocabulary-muted">{t("hotwordPriority.noEvents")}</p>}
    <button type="button" className="btn-ghost" disabled={disabled} onClick={onReset}>{t("hotwordPriority.reset")}</button>
  </section>;
}
