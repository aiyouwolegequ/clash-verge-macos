import { useQuery } from '@tanstack/react-query'
import { listen } from '@tauri-apps/api/event'
import React, { useCallback, useEffect, useMemo, useRef } from 'react'
import {
  BaseConfig,
  getBaseConfig,
  getRuleProviders,
  getRules,
} from 'tauri-plugin-mihomo-api'

import { useVerge } from '@/hooks/use-verge'
import {
  calcuProxies,
  calcuProxyProviders,
  getAppUptime,
  getRunningMode,
  getSystemProxy,
} from '@/services/cmds'
import { queryClient } from '@/services/query-client'

import {
  ClashConfigContext,
  CoreDataStatusContext,
  ProxiesContext,
  RefreshersContext,
  RulesContext,
  SystemContext,
  UptimeContext,
} from './app-data-context'

const TQ_MIHOMO = {
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  staleTime: 1500,
  retry: 3,
  retryDelay: (attempt: number) => Math.min(200 * 2 ** attempt, 3000),
} as const

const TQ_DEFAULTS = {
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  staleTime: 5000,
  retry: 2,
} as const

const LAST_USABLE_DATA_GRACE_MS = 5000

function useStableFn<T extends (...args: any[]) => any>(fn: T): T {
  const ref = useRef(fn)
  ref.current = fn
  return useCallback((...args: Parameters<T>) => ref.current(...args), []) as T
}

type ProxiesData = Awaited<ReturnType<typeof calcuProxies>>

function useLastUsableData<T>(
  data: T | undefined,
  isUsable: (value: T | undefined) => value is T,
  graceMs = LAST_USABLE_DATA_GRACE_MS,
): T | undefined {
  const lastUsableRef = useRef<T | undefined>(undefined)
  const unusableSinceRef = useRef<number | undefined>(undefined)
  const [useFallback, setUseFallback] = React.useReducer(
    (_: boolean, next: boolean) => next,
    false,
  )
  const usable = isUsable(data)

  React.useLayoutEffect(() => {
    if (usable) {
      lastUsableRef.current = data
      unusableSinceRef.current = undefined
      setUseFallback(false)
      return
    }

    if (!lastUsableRef.current) {
      unusableSinceRef.current = undefined
      setUseFallback(false)
      return
    }

    const unusableSince = unusableSinceRef.current ?? Date.now()
    unusableSinceRef.current = unusableSince
    const remainingMs = graceMs - (Date.now() - unusableSince)
    if (remainingMs <= 0) {
      setUseFallback(false)
      return
    }

    setUseFallback(true)
    const timer = window.setTimeout(() => setUseFallback(false), remainingMs)
    return () => window.clearTimeout(timer)
  }, [data, graceMs, isUsable, usable])

  if (usable) {
    return data
  }

  if (useFallback && lastUsableRef.current) {
    return lastUsableRef.current
  }

  return data
}

const hasUsableProxiesData = (
  data: ProxiesData | undefined,
): data is ProxiesData => {
  if (!data) return false

  return Boolean(
    data.groups?.length ||
      data.global?.all?.length ||
      data.proxies?.some(
        (proxy) => proxy?.name && !['DIRECT', 'REJECT'].includes(proxy.name),
      ),
  )
}

const hasUsableClashConfig = (
  data: BaseConfig | undefined,
): data is BaseConfig => data != null

// 全局数据提供者组件
export const AppDataProvider = ({
  children,
}: {
  children: React.ReactNode
}) => {
  const { verge } = useVerge()

  const {
    data: rawProxiesData,
    isPending: rawIsProxiesPending,
    refetch: _refetchProxy,
  } = useQuery({
    queryKey: ['getProxies'],
    queryFn: calcuProxies,
    ...TQ_MIHOMO,
  })

  const {
    data: rawClashConfig,
    isPending: rawIsClashConfigPending,
    refetch: _refetchClashConfig,
  } = useQuery({
    queryKey: ['getClashConfig'],
    queryFn: getBaseConfig,
    ...TQ_MIHOMO,
  })

  const proxiesData = useLastUsableData(rawProxiesData, hasUsableProxiesData)
  const clashConfig = useLastUsableData(rawClashConfig, hasUsableClashConfig)
  const isProxiesPending = rawIsProxiesPending && !proxiesData
  const isClashConfigPending = rawIsClashConfigPending && !clashConfig

  const { data: proxyProviders, refetch: _refetchProxyProviders } = useQuery({
    queryKey: ['getProxyProviders'],
    queryFn: calcuProxyProviders,
    ...TQ_MIHOMO,
  })

  const { data: ruleProviders, refetch: _refetchRuleProviders } = useQuery({
    queryKey: ['getRuleProviders'],
    queryFn: getRuleProviders,
    ...TQ_MIHOMO,
  })

  const { data: rulesData, refetch: _refetchRules } = useQuery({
    queryKey: ['getRules'],
    queryFn: getRules,
    ...TQ_MIHOMO,
  })

  const { data: sysproxy, refetch: _refetchSysproxy } = useQuery({
    queryKey: ['getSystemProxy'],
    queryFn: getSystemProxy,
    ...TQ_DEFAULTS,
  })

  const { data: runningMode } = useQuery({
    queryKey: ['getRunningMode'],
    queryFn: getRunningMode,
    ...TQ_DEFAULTS,
  })

  const { data: uptimeData } = useQuery({
    queryKey: ['appUptime'],
    queryFn: getAppUptime,
    ...TQ_DEFAULTS,
    refetchInterval: 3000,
    retry: 1,
  })

  const refreshProxy = useStableFn(_refetchProxy)
  const refreshClashConfig = useStableFn(_refetchClashConfig)
  const refreshRules = useStableFn(_refetchRules)
  const refreshSysproxy = useStableFn(_refetchSysproxy)
  const refreshProxyProviders = useStableFn(_refetchProxyProviders)
  const refreshRuleProviders = useStableFn(_refetchRuleProviders)

  useEffect(() => {
    let lastProfileId: string | null = null
    let lastProfileUpdateTime = 0
    let lastProxyRefreshTime = 0
    const refreshThrottle = 800
    const cleanupFns: Array<() => void> = []
    let disposed = false

    const addCleanup = (fn: () => void) => {
      if (disposed) {
        try {
          fn()
        } catch (error) {
          console.error('[DataProvider] Cleanup error:', error)
        }
        return
      }
      cleanupFns.push(fn)
    }

    const handleProfileChanged = (event: { payload: string }) => {
      const newProfileId = event.payload
      const now = Date.now()
      if (
        lastProfileId === newProfileId &&
        now - lastProfileUpdateTime < refreshThrottle
      ) {
        return
      }
      lastProfileId = newProfileId
      lastProfileUpdateTime = now
      void queryClient.invalidateQueries({ queryKey: ['getProfiles'] })
      refreshRules().catch(() => {})
      refreshRuleProviders().catch(() => {})
    }

    const handleRefreshProxy = () => {
      const now = Date.now()
      if (now - lastProxyRefreshTime <= refreshThrottle) return
      lastProxyRefreshTime = now
      refreshProxy().catch(() => {})
    }

    const initializeListeners = async () => {
      try {
        const unlistenProfile = await listen<string>(
          'profile-changed',
          handleProfileChanged,
        )
        addCleanup(unlistenProfile)
      } catch (error) {
        console.error('[AppDataProvider] 监听 Profile 事件失败:', error)
      }

      try {
        const unlistenProxy = await listen(
          'verge://refresh-proxy-config',
          handleRefreshProxy,
        )
        addCleanup(unlistenProxy)
      } catch (error) {
        console.warn('[AppDataProvider] 设置 Tauri 事件监听器失败:', error)
      }
    }

    void initializeListeners()

    return () => {
      disposed = true
      cleanupFns.forEach((fn) => {
        try {
          fn()
        } catch (error) {
          console.error('[DataProvider] Cleanup error:', error)
        }
      })
    }
  }, [refreshProxy, refreshRules, refreshRuleProviders])

  const refreshAll = useCallback(async () => {
    await Promise.all([
      refreshProxy(),
      refreshClashConfig(),
      refreshRules(),
      refreshSysproxy(),
      refreshProxyProviders(),
      refreshRuleProviders(),
    ])
  }, [
    refreshProxy,
    refreshClashConfig,
    refreshRules,
    refreshSysproxy,
    refreshProxyProviders,
    refreshRuleProviders,
  ])

  const proxiesValue = useMemo(
    () => ({
      proxies: proxiesData,
      proxyProviders: proxyProviders || {},
      isProxiesPending,
    }),
    [proxiesData, proxyProviders, isProxiesPending],
  )

  const rulesValue = useMemo(
    () => ({
      rules: rulesData?.rules ?? [],
      ruleProviders: ruleProviders?.providers || {},
    }),
    [rulesData, ruleProviders],
  )

  const clashConfigValue = useMemo(
    () => ({
      clashConfig,
      isClashConfigPending,
    }),
    [clashConfig, isClashConfigPending],
  )

  const systemValue = useMemo(() => {
    const calculateSystemProxyAddress = () => {
      if (!verge || !clashConfig) return '-'

      const isPacMode = verge.proxy_auto_config ?? false

      if (isPacMode) {
        // PAC模式：显示我们期望设置的代理地址
        const proxyHost = verge.proxy_host || '127.0.0.1'
        const proxyPort =
          verge.verge_mixed_port || clashConfig.mixedPort || 7897
        return `${proxyHost}:${proxyPort}`
      } else {
        // HTTP代理模式：优先使用系统地址，但如果格式不正确则使用期望地址
        const systemServer = sysproxy?.server
        if (
          systemServer &&
          systemServer !== '-' &&
          !systemServer.startsWith(':')
        ) {
          return systemServer
        } else {
          // 系统地址无效，返回期望的代理地址
          const proxyHost = verge.proxy_host || '127.0.0.1'
          const proxyPort =
            verge.verge_mixed_port || clashConfig.mixedPort || 7897
          return `${proxyHost}:${proxyPort}`
        }
      }
    }

    return {
      sysproxy,
      runningMode,
      systemProxyAddress: calculateSystemProxyAddress(),
    }
  }, [sysproxy, runningMode, verge, clashConfig])

  const uptimeValue = useMemo(() => ({ uptime: uptimeData || 0 }), [uptimeData])

  const coreDataStatusValue = useMemo(
    () => ({ isCoreDataPending: isProxiesPending || isClashConfigPending }),
    [isProxiesPending, isClashConfigPending],
  )

  const refreshersValue = useMemo(
    () => ({
      refreshProxy,
      refreshClashConfig,
      refreshRules,
      refreshSysproxy,
      refreshProxyProviders,
      refreshRuleProviders,
      refreshAll,
    }),
    [
      refreshProxy,
      refreshClashConfig,
      refreshRules,
      refreshSysproxy,
      refreshProxyProviders,
      refreshRuleProviders,
      refreshAll,
    ],
  )

  return (
    <ProxiesContext value={proxiesValue}>
      <RulesContext value={rulesValue}>
        <ClashConfigContext value={clashConfigValue}>
          <SystemContext value={systemValue}>
            <UptimeContext value={uptimeValue}>
              <CoreDataStatusContext value={coreDataStatusValue}>
                <RefreshersContext value={refreshersValue}>
                  {children}
                </RefreshersContext>
              </CoreDataStatusContext>
            </UptimeContext>
          </SystemContext>
        </ClashConfigContext>
      </RulesContext>
    </ProxiesContext>
  )
}
