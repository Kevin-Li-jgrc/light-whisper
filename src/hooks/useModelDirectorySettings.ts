import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { getModelsDir, pickFolder, setModelsDir } from "@/api/tauri";

interface ModelMigrationStatusPayload {
  status: string;
  message?: string;
}

interface UseModelDirectorySettingsOptions {
  retryModel: () => void;
}

export function useModelDirectorySettings({ retryModel }: UseModelDirectorySettingsOptions) {
  const { t } = useTranslation();
  const [modelsDir, setModelsDirState] = useState("");
  const [modelsDirCustom, setModelsDirCustom] = useState(false);
  const [modelsDirMigrating, setModelsDirMigrating] = useState(false);
  const [modelsMigrateMsg, setModelsMigrateMsg] = useState("");

  const refreshModelsDir = useCallback(async () => {
    const info = await getModelsDir();
    setModelsDirState(info.path);
    setModelsDirCustom(info.is_custom);
  }, []);

  useEffect(() => {
    void refreshModelsDir().catch(() => {});
  }, [refreshModelsDir]);

  useEffect(() => {
    const unlisten = listen<ModelMigrationStatusPayload>(
      "models-migrate-status",
      (event) => {
        const { status, message } = event.payload;
        if (status === "migrating" && message) {
          setModelsMigrateMsg(message);
        } else if (status === "completed") {
          setModelsMigrateMsg("");
        }
      },
    );
    return () => { unlisten.then((dispose) => dispose()); };
  }, []);

  const handleRestoreDefaultModelsDir = useCallback(async () => {
    try {
      setModelsDirMigrating(true);
      const result = await setModelsDir(null, false);
      try {
        await refreshModelsDir();
      } catch (refreshError) {
        console.error("Failed to refresh model directory after successful reset:", refreshError);
      }
      if (result.runtimeWarning) {
        toast.info(result.runtimeWarning);
      } else {
        toast.success(t("toast.modelsDirResetDefault"));
      }
      retryModel();
    } catch (error) {
      try {
        await refreshModelsDir();
      } catch (refreshError) {
        console.error("Failed to refresh model directory after reset error:", refreshError);
      }
      retryModel();
      toast.error(error instanceof Error ? error.message : t("toast.modelsDirResetFailed"));
    } finally {
      setModelsDirMigrating(false);
    }
  }, [refreshModelsDir, retryModel, t]);

  const handleChooseModelsDir = useCallback(async () => {
    try {
      const folder = await pickFolder();
      if (!folder) return;
      setModelsDirMigrating(true);
      const result = await setModelsDir(folder, true);
      try {
        await refreshModelsDir();
      } catch (refreshError) {
        console.error("Failed to refresh model directory after successful migration:", refreshError);
      }
      if (result.runtimeWarning) {
        toast.info(result.runtimeWarning);
      } else {
        toast.success(t("toast.modelsDirUpdated"));
      }
      retryModel();
    } catch (error) {
      try {
        await refreshModelsDir();
      } catch (refreshError) {
        console.error("Failed to refresh model directory after migration error:", refreshError);
      }
      retryModel();
      toast.error(error instanceof Error ? error.message : t("toast.modelsDirChangeFailed"));
    } finally {
      setModelsDirMigrating(false);
      setModelsMigrateMsg("");
    }
  }, [refreshModelsDir, retryModel, t]);

  return {
    handleChooseModelsDir,
    handleRestoreDefaultModelsDir,
    modelsDir,
    modelsDirCustom,
    modelsDirMigrating,
    modelsMigrateMsg,
  };
}
