import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";

const statuses = ["sent", "busy", "empty", "releaseKeys", "focusChanged", "failed"] as const;
type Status = typeof statuses[number];

export default function ReinsertNotice() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<Status | null>(null);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const listener = listen<string>("reinsert-status", ({ payload }) => {
        if (!active || !statuses.includes(payload as Status)) return;
        clearTimeout(timer);
        setStatus(payload as Status);
        timer = setTimeout(() => setStatus(null), 2400);
      }).catch(() => () => {});
    return () => {
      active = false;
      clearTimeout(timer);
      void listener.then((unlisten) => unlisten());
    };
  }, []);
  return status ? <div className="reinsert-notice" role="status">{t(`reinsert.${status}`)}</div> : null;
}
