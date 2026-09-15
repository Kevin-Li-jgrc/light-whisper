import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  getAlibabaAsrConfig,
  getEngine,
  getOnlineAsrApiKey,
  getOnlineAsrEndpoint,
  listAlibabaAsrModels,
  setAlibabaAsrModel,
  setEngine,
  setOnlineAsrApiKey,
  setOnlineAsrEndpoint,
} from "@/api/tauri";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";

interface UseAsrEngineSettingsOptions {
  engineLabel: (engine: string) => string;
  retryModel: () => void;
}

export function useAsrEngineSettings({
  engineLabel,
  retryModel,
}: UseAsrEngineSettingsOptions) {
  const { t } = useTranslation();
  const [engine, setEngineState] = useState("qwen3-asr-0.6b");
  const [engineLoading, setEngineLoading] = useState(true);
  const [onlineAsrApiKey, setOnlineAsrApiKeyState] = useState("");
  const [onlineAsrRegion, setOnlineAsrRegion] = useState("international");
  const [onlineAsrUrl, setOnlineAsrUrl] = useState("");
  const [onlineAsrRegionLoading, setOnlineAsrRegionLoading] = useState(false);
  const [alibabaAsrModel, setAlibabaAsrModelState] = useState("qwen3-asr-flash");
  const [alibabaAsrModels, setAlibabaAsrModelsState] = useState<readonly string[]>([]);
  const [alibabaAsrModelsSource, setAlibabaAsrModelsSource] = useState<"live" | "fallback">("fallback");
  const [alibabaAsrModelsLoading, setAlibabaAsrModelsLoading] = useState(false);

  const computeOnlineAsrKeyringUser = useCallback((engineValue: string, region: string): string => {
    if (engineValue === "alibaba-asr") {
      return region === "domestic" ? "alibaba-asr-cn-api-key" : "alibaba-asr-intl-api-key";
    }
    return "glm-asr-api-key";
  }, []);

  const onlineAsrKeySave = useDebouncedCallback(
    async (value: string, keyringUser: string) => {
      try {
        await setOnlineAsrApiKey(value, keyringUser);
      } catch (error) {
        toast.error(t("toast.onlineAsrKeySaveFailed"));
        throw error;
      }
    },
    600,
    { onUnmount: "flush" },
  );

  useEffect(() => {
    getEngine().then((value) => {
      setEngineState(value);
      setEngineLoading(false);
    }).catch(() => setEngineLoading(false));
    getOnlineAsrApiKey().then((key) => setOnlineAsrApiKeyState(key || "")).catch(() => {});
    getOnlineAsrEndpoint().then((endpoint) => {
      setOnlineAsrRegion(endpoint.region);
      setOnlineAsrUrl(endpoint.url);
    }).catch(() => {});
    getAlibabaAsrConfig().then((config) => {
      setAlibabaAsrModelState(config.model);
      setAlibabaAsrModelsState(config.models);
    }).catch(() => {});
  }, []);

  const refreshAlibabaModels = useCallback(async () => {
    setAlibabaAsrModelsLoading(true);
    try {
      const result = await listAlibabaAsrModels();
      if (result.models.length > 0) {
        setAlibabaAsrModelsState(result.models);
        setAlibabaAsrModelsSource(result.source);
      }
    } catch {
      // Keep the last fallback/live list when the refresh cannot reach the service.
    } finally {
      setAlibabaAsrModelsLoading(false);
    }
  }, []);

  const alibabaHasKey = engine === "alibaba-asr" && onlineAsrApiKey.trim().length > 0;
  useEffect(() => {
    if (engine !== "alibaba-asr" || !alibabaHasKey) return;
    void refreshAlibabaModels();
  }, [alibabaHasKey, engine, onlineAsrRegion, refreshAlibabaModels]);

  const handleEngineSwitch = useCallback(async (newEngine: string) => {
    if (engineLoading || newEngine === engine) return;
    setEngineLoading(true);
    // Flush the old engine's key before set_engine so the two writes keep their order.
    try {
      try {
        await onlineAsrKeySave.flush();
      } catch {
        return;
      }
      await setEngine(newEngine);
      setEngineState(newEngine);
      toast.success(t("toast.switchedToEngine", { label: engineLabel(newEngine) }));
      // The backend reloads the keyring slot for the new online engine.
      if (newEngine === "glm-asr" || newEngine === "alibaba-asr") {
        try {
          const [key, endpoint] = await Promise.all([
            getOnlineAsrApiKey(),
            getOnlineAsrEndpoint(),
          ]);
          setOnlineAsrApiKeyState(key || "");
          setOnlineAsrRegion(endpoint.region);
          setOnlineAsrUrl(endpoint.url);
        } catch {
          // Keep the current fields when secure storage is unavailable.
        }
      }
      retryModel();
    } catch {
      toast.error(t("toast.switchEngineFailed"));
    } finally {
      setEngineLoading(false);
    }
  }, [engine, engineLabel, engineLoading, onlineAsrKeySave, retryModel, t]);

  const handleOnlineAsrRegionChange = useCallback(async (region: string) => {
    if (onlineAsrRegionLoading || onlineAsrRegion === region) return;
    setOnlineAsrRegionLoading(true);
    try {
      try {
        await onlineAsrKeySave.flush();
      } catch {
        return;
      }
      const endpoint = await setOnlineAsrEndpoint(region);
      setOnlineAsrRegion(endpoint.region);
      setOnlineAsrUrl(endpoint.url);
      if (engine === "alibaba-asr") {
        try {
          const key = await getOnlineAsrApiKey();
          setOnlineAsrApiKeyState(key || "");
        } catch {
          // Preserve the previous key display if secure storage is unavailable.
        }
      }
    } catch {
      toast.error(t("toast.onlineAsrRegionSwitchFailed"));
    } finally {
      setOnlineAsrRegionLoading(false);
    }
  }, [engine, onlineAsrKeySave, onlineAsrRegion, onlineAsrRegionLoading, t]);

  const handleOnlineAsrApiKeyChange = useCallback((value: string) => {
    setOnlineAsrApiKeyState(value);
    onlineAsrKeySave.schedule(
      value,
      computeOnlineAsrKeyringUser(engine, onlineAsrRegion),
    );
  }, [computeOnlineAsrKeyringUser, engine, onlineAsrKeySave, onlineAsrRegion]);

  const handleAlibabaAsrModelSelect = useCallback(async (model: string) => {
    try {
      await setAlibabaAsrModel(model);
      setAlibabaAsrModelState(model);
    } catch {
      // Keep the previous model when persistence fails.
    }
  }, []);

  return {
    alibabaAsrModel,
    alibabaAsrModels,
    alibabaAsrModelsLoading,
    alibabaAsrModelsSource,
    alibabaHasKey,
    engine,
    engineLoading,
    handleAlibabaAsrModelSelect,
    handleEngineSwitch,
    handleOnlineAsrApiKeyChange,
    handleOnlineAsrRegionChange,
    onlineAsrApiKey,
    onlineAsrRegion,
    onlineAsrRegionLoading,
    onlineAsrUrl,
    refreshAlibabaModels,
  };
}
