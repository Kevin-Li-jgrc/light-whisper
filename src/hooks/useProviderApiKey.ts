import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { deleteProviderApiKey, saveProviderApiKey } from "@/api/tauri";
import { useDebouncedCallback } from "./useDebouncedCallback";

// 读取只更新显示；只有用户编辑或明确删除时才写入系统密钥环。
export function useProviderApiKey(provider: string, read: (provider: string) => Promise<string>, enabled = true) {
  const { t } = useTranslation();
  const [value, setValue] = useState({ provider, key: "", loading: true });
  const providerRef = useRef(provider);
  providerRef.current = provider;
  const requestId = useRef(0);
  const save = useDebouncedCallback(async (target: string, key: string) => {
    try {
      await saveProviderApiKey(target, key);
    } catch (error) {
      toast.error(t("toast.apiKeySaveFailed"));
      throw error;
    }
  }, 600, { onUnmount: "flush" });

  const refresh = useCallback(async () => {
    const target = providerRef.current;
    const id = ++requestId.current;
    setValue((previous) => ({ provider: target, key: previous.provider === target ? previous.key : "", loading: true }));
    try {
      const key = await read(target);
      if (requestId.current === id && providerRef.current === target) {
        setValue({ provider: target, key, loading: false });
      }
    } catch {
      if (requestId.current === id && providerRef.current === target) {
        setValue((previous) => ({ ...previous, loading: false }));
        toast.error(t("toast.apiKeyReadFailed"));
      }
    }
  }, [read, t]);

  useEffect(() => {
    if (enabled) void refresh();
    return () => {
      ++requestId.current;
      // 切换提供商时提交旧输入，避免新输入取消旧提供商尚未执行的保存。
      void save.flush().catch(() => undefined);
    };
  }, [enabled, provider, refresh, save]);

  const setKey = useCallback((key: string) => {
    ++requestId.current;
    const target = providerRef.current;
    setValue({ provider: target, key, loading: false });
    // 清空输入框可以用于替换密钥，但不能隐式删除已保存的凭据。
    save.cancel();
    if (key.trim()) save.schedule(target, key.trim());
  }, [save]);

  const remove = useCallback(async () => {
    const target = providerRef.current;
    ++requestId.current;
    save.cancel();
    setValue((previous) => ({ ...previous, loading: true }));
    try {
      // 等待已开始的保存完成，防止删除后被旧请求重新写回。
      await save.flush().catch(() => undefined);
      await deleteProviderApiKey(target);
      if (providerRef.current === target) setValue({ provider: target, key: "", loading: false });
    } catch {
      if (providerRef.current === target) setValue((previous) => ({ ...previous, loading: false }));
      toast.error(t("toast.apiKeyDeleteFailed"));
    }
  }, [save, t]);

  return {
    key: value.provider === provider ? value.key : "",
    loading: value.provider !== provider || value.loading,
    setKey, refresh, remove, flush: save.flush,
  };
}
