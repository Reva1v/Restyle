import { commands, type Settings } from "../../lib/ipc";
import type { T } from "../../lib/i18n";
import { Button, Row, SectionTitle, ToggleRow } from "../../ui/kit";

const HISTORY_MIN = 1;
const HISTORY_MAX = 100;

/**
 * «Вставка и история» (макет, раздел 06) + выбор темы и языка интерфейса.
 */
export default function PasteHistory({
  t,
  settings,
  apply,
}: {
  t: T;
  settings: Settings;
  apply: (s: Settings) => void;
}) {
  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto pr-1">
      <SectionTitle title={t("paste.title")} hint={t("paste.hint")} />
      <div className="flex flex-col gap-2">
        <ToggleRow
          title={t("paste.auto")}
          hint={t("paste.autoHint")}
          checked={settings.autoPaste}
          onChange={(v) => apply({ ...settings, autoPaste: v })}
        />
        <ToggleRow
          title={t("paste.restore")}
          hint={t("paste.restoreHint")}
          checked={settings.restoreClipboard}
          onChange={(v) => apply({ ...settings, restoreClipboard: v })}
        >
          <span className="font-mono text-[11.5px] text-paper-soft dark:text-ink-text/45">150 мс</span>
        </ToggleRow>
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <SectionTitle title={t("history.title")} hint={t("history.hint")} />
      <div className="flex flex-col gap-2">
        <Row title={t("history.limit")}>
          <span className="flex items-center gap-2.5">
            <input
              type="range"
              min={HISTORY_MIN}
              max={HISTORY_MAX}
              value={settings.historyLimit}
              onChange={(e) => apply({ ...settings, historyLimit: Number(e.target.value) })}
              className="h-1 w-[160px] cursor-pointer appearance-none rounded bg-black/[.12] accent-amber-deep dark:bg-white/15 dark:accent-amber"
            />
            <span className="w-6 text-right font-mono text-[12.5px] font-medium text-paper-text dark:text-ink-text">
              {settings.historyLimit}
            </span>
          </span>
        </Row>
        <ToggleRow
          title={t("history.toDisk")}
          hint={t("history.toDiskHint")}
          checked={settings.historyToDisk}
          onChange={(v) => apply({ ...settings, historyToDisk: v })}
        />
        <div className="flex gap-2">
          <Button onClick={() => void commands.openHistoryWindow()}>{t("app.history")}</Button>
          <Button kind="danger" onClick={() => void commands.clearHistory()}>{t("common.clear")}</Button>
        </div>
      </div>

    </div>
  );
}
