import { useRef } from "react";
import useSWR, { mutate } from "swr";
import { closeAllConnections } from "tauri-plugin-mihomo-api";

import { useVerge } from "@/hooks/use-verge";
import { useAppData } from "@/providers/app-data-context";
import { getAutotemProxy } from "@/services/cmds";

// 系统代理状态检测统一逻辑
export const useSystemProxyState = () => {
  const { verge, mutateVerge, patchVerge } = useVerge();
  const { sysproxy } = useAppData();
  const { data: autoproxy } = useSWR("getAutotemProxy", getAutotemProxy, {
    revalidateOnFocus: true,
    revalidateOnReconnect: true,
  });

  const { enable_system_proxy, proxy_auto_config } = verge ?? {};

  const getSystemProxyActualState = () => {
    const userEnabled = enable_system_proxy ?? false;

    // 用户配置状态应该与系统实际状态一致
    // 如果用户启用了系统代理，检查实际的系统状态
    if (userEnabled) {
      if (proxy_auto_config) {
        return autoproxy?.enable ?? false;
      } else {
        return sysproxy?.enable ?? false;
      }
    }

    // 用户没有启用时，返回 false
    return false;
  };

  const getSystemProxyIndicator = () => {
    if (proxy_auto_config) {
      return autoproxy?.enable ?? false;
    } else {
      return sysproxy?.enable ?? false;
    }
  };

  // "最后一次生效"模式：快速连续点击时，只执行最终状态
  const pendingRef = useRef<boolean | null>(null);
  const busyRef = useRef(false);

  const toggleSystemProxy = async (enabled: boolean) => {
    mutateVerge({ ...verge, enable_system_proxy: enabled }, false);
    pendingRef.current = enabled;

    if (busyRef.current) return;
    busyRef.current = true;

    try {
      while (pendingRef.current !== null) {
        const target = pendingRef.current;
        pendingRef.current = null;
        if (!target && verge?.auto_close_connection) {
          await closeAllConnections().catch(() => {});
        }
        await patchVerge({ enable_system_proxy: target });
      }
    } finally {
      busyRef.current = false;
      await Promise.all([mutate("getSystemProxy"), mutate("getAutotemProxy")]);
    }
  };

  return {
    actualState: getSystemProxyActualState(),
    indicator: getSystemProxyIndicator(),
    configState: enable_system_proxy ?? false,
    sysproxy,
    autoproxy,
    proxy_auto_config,
    toggleSystemProxy,
  };
};
