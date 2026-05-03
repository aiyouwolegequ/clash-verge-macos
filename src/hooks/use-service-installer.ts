import { useCallback } from 'react'

import { installService, restartCore } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import { useSystemState } from './use-system-state'

export const useServiceInstaller = () => {
  const { mutateSystemState } = useSystemState()

  const installServiceAndRestartCore = useCallback(async () => {
    // 步骤 1：安装服务
    try {
      showNotice.info('settings.statuses.clashService.installing')
      await installService()
      showNotice.success(
        'settings.feedback.notifications.clashService.installSuccess',
      )
    } catch (err) {
      showNotice.error(err)
      // 安装失败后仍刷新状态，让 UI 反映真实状况
      await mutateSystemState()
      return
    }

    // 步骤 2：重启核心以使用服务模式
    try {
      showNotice.info('settings.statuses.clash.restarting')
      await restartCore()
      showNotice.success('settings.feedback.notifications.clash.restartSuccess')
    } catch (err) {
      showNotice.error(err)
    }

    // 步骤 3：刷新系统状态
    await mutateSystemState()
  }, [mutateSystemState])

  return { installServiceAndRestartCore }
}
