import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
  listInputDevices,
  setInputDevice,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
  testMicrophone,
} from "@/api/tauri";
import type { InputDeviceInfo } from "@/types";
import { INPUT_DEVICE_STORAGE_KEY, MIC_LEVEL_MONITOR_ENABLED_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { useTranslation } from "react-i18next";

interface MicrophoneLevelPayload {
  deviceName?: string;
  level?: number;
}

interface UseMicrophoneSettingsOptions {
  active: boolean;
  closePicker: () => void;
  isRecording: boolean;
}

export function useMicrophoneSettings({
  active,
  closePicker,
  isRecording,
}: UseMicrophoneSettingsOptions) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const [inputDevices, setInputDevices] = useState<InputDeviceInfo[]>([]);
  const [selectedInputDeviceName, setSelectedInputDeviceName] = useState("");
  const [deviceListLoading, setDeviceListLoading] = useState(true);
  const [micLevel, setMicLevel] = useState(0);
  const [micMonitorReady, setMicMonitorReady] = useState(false);
  const [micLevelMonitorEnabled, setMicLevelMonitorEnabled] = useState(
    () => readLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY) === "true",
  );

  const refreshInputDevices = useCallback(async () => {
    setDeviceListLoading(true);
    try {
      const payload = await listInputDevices();
      setInputDevices(payload.devices);
      setSelectedInputDeviceName(payload.selectedDeviceName ?? "");
    } catch {
      toast.error(tRef.current("toast.micListFailed"));
    } finally {
      setDeviceListLoading(false);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const stored = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
      if (stored) {
        await setInputDevice(stored).catch(() => {});
      }
      await refreshInputDevices();
    })();
  }, [refreshInputDevices]);

  useEffect(() => {
    let disposed = false;
    let unlisten: null | (() => void) = null;

    const startMonitor = async () => {
      try {
        await stopMicrophoneLevelMonitor().catch(() => undefined);
        if (!active || !micLevelMonitorEnabled || isRecording) {
          if (!disposed) {
            setMicMonitorReady(false);
            setMicLevel(0);
          }
          return;
        }
        await startMicrophoneLevelMonitor();
        if (!disposed) setMicMonitorReady(true);
      } catch {
        if (!disposed) {
          setMicMonitorReady(false);
          setMicLevel(0);
        }
      }
    };

    void (async () => {
      try {
        unlisten = await listen<MicrophoneLevelPayload>("microphone-level", (event) => {
          if (disposed) return;
          const level = typeof event.payload?.level === "number" ? event.payload.level : 0;
          setMicLevel(Math.max(0, Math.min(1, level)));
        });
      } catch {
        // Ignore event subscription failures and keep the controls usable.
      }

      await startMonitor();

      if (disposed && unlisten) {
        unlisten();
        unlisten = null;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      void stopMicrophoneLevelMonitor().catch(() => undefined);
    };
  }, [active, isRecording, micLevelMonitorEnabled, selectedInputDeviceName]);

  const handleInputDeviceChange = useCallback(async (name: string) => {
    closePicker();
    setDeviceListLoading(true);
    try {
      await setInputDevice(name || null);
      writeLocalStorage(INPUT_DEVICE_STORAGE_KEY, name || "");
      setSelectedInputDeviceName(name);
      await refreshInputDevices();
    } catch {
      toast.error(t("toast.micSwitchFailed"));
    } finally {
      setDeviceListLoading(false);
    }
  }, [closePicker, refreshInputDevices, t]);

  const handleMicLevelMonitorToggle = useCallback((enabled: boolean) => {
    setMicLevelMonitorEnabled(enabled);
    writeLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY, enabled ? "true" : "false");
    if (!enabled) {
      setMicMonitorReady(false);
      setMicLevel(0);
    }
  }, []);

  const handleTestMicrophone = useCallback(async () => {
    try {
      setMicMonitorReady(false);
      setMicLevel(0);
      await stopMicrophoneLevelMonitor().catch(() => undefined);
      const message = await testMicrophone();
      toast.success(message);
      if (micLevelMonitorEnabled && !isRecording) {
        await startMicrophoneLevelMonitor();
        setMicMonitorReady(true);
      }
    } catch {
      toast.error(t("toast.micTestFailed"));
    }
  }, [isRecording, micLevelMonitorEnabled, t]);

  return {
    deviceListLoading,
    handleInputDeviceChange,
    handleMicLevelMonitorToggle,
    handleTestMicrophone,
    inputDevices,
    micLevel,
    micLevelMonitorEnabled,
    micMonitorReady,
    refreshInputDevices,
    selectedInputDeviceName,
  };
}
