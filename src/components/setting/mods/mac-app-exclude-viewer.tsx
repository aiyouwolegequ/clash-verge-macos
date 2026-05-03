import { Refresh as RefreshIcon } from '@mui/icons-material'
import {
  Box,
  Checkbox,
  FormControlLabel,
  IconButton,
  List,
  ListItem,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  TextField,
  Tooltip,
} from '@mui/material'
import { forwardRef, useImperativeHandle, useState } from 'react'
import { createPortal } from 'react-dom'

import { BaseDialog } from '@/components/base'
import { useVerge } from '@/hooks/use-verge'
import { getMacosApps, refreshMacExcludeApps } from '@/services/cmds'

export interface MacAppExcludeViewerRef {
  open: () => void
}

export const MacAppExcludeViewer = forwardRef<MacAppExcludeViewerRef>(
  (_, ref) => {
    const [open, setOpen] = useState(false)
    const [search, setSearch] = useState('')
    const [apps, setApps] = useState<{ name: string; path: string }[]>([])
    const [refreshing, setRefreshing] = useState(false)
    const { verge, patchVerge, mutateVerge } = useVerge()
    const excludeApps = verge?.mac_exclude_apps || []

    useImperativeHandle(ref, () => ({
      open: async () => {
        setOpen(true)
        const appList = await getMacosApps()
        setApps(appList)

        if (excludeApps.length === 0 && appList.length > 0) {
          const allPaths = appList.map((a) => a.path)
          patchVerge({ mac_exclude_apps: allPaths })
          mutateVerge({ ...verge, mac_exclude_apps: allPaths } as any, false)
        }
      },
    }))

    const onToggle = (path: string) => {
      const newExcludes = excludeApps.includes(path)
        ? excludeApps.filter((a) => a !== path)
        : [...excludeApps, path]
      patchVerge({ mac_exclude_apps: newExcludes })
      mutateVerge({ ...verge, mac_exclude_apps: newExcludes } as any, false)
    }

    const filteredApps = apps.filter(
      (app) =>
        app.name.toLowerCase().includes(search.toLowerCase()) ||
        app.path.toLowerCase().includes(search.toLowerCase()),
    )

    const isAllSelected =
      filteredApps.length > 0 &&
      filteredApps.every((a) => excludeApps.includes(a.path))
    const isIndeterminate =
      !isAllSelected && filteredApps.some((a) => excludeApps.includes(a.path))

    const toggleAll = () => {
      let newExcludes = [...excludeApps]
      if (isAllSelected) {
        newExcludes = newExcludes.filter(
          (path) => !filteredApps.some((a) => a.path === path),
        )
      } else {
        const toAdd = filteredApps
          .filter((a) => !newExcludes.includes(a.path))
          .map((a) => a.path)
        newExcludes = [...newExcludes, ...toAdd]
      }
      patchVerge({ mac_exclude_apps: newExcludes })
      mutateVerge({ ...verge, mac_exclude_apps: newExcludes } as any, false)
    }

    const onRefresh = async () => {
      setRefreshing(true)
      try {
        await refreshMacExcludeApps()
      } finally {
        setRefreshing(false)
      }
    }

    return createPortal(
      <BaseDialog
        open={open}
        onClose={() => setOpen(false)}
        title="macOS 直连应用"
        okBtn="确定"
        onOk={() => {
          setOpen(false)
          if (excludeApps.length > 0) refreshMacExcludeApps()
        }}
        disableCancel
        contentSx={{
          width: 450,
          height: 600,
          p: 0,
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <Box
          sx={{
            p: 2,
            borderBottom: 1,
            borderColor: 'divider',
            display: 'flex',
            flexDirection: 'column',
            gap: 1,
          }}
        >
          <Box sx={{ display: 'flex', gap: 1, alignItems: 'center' }}>
            <TextField
              size="small"
              placeholder="搜索应用..."
              variant="outlined"
              fullWidth
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            <Tooltip title="刷新应用列表">
              <IconButton
                onClick={onRefresh}
                disabled={refreshing}
                size="small"
              >
                <RefreshIcon
                  sx={{
                    animation: refreshing
                      ? 'spin 1s linear infinite'
                      : undefined,
                    '@keyframes spin': {
                      '0%': { transform: 'rotate(0deg)' },
                      '100%': { transform: 'rotate(360deg)' },
                    },
                  }}
                />
              </IconButton>
            </Tooltip>
          </Box>
          <FormControlLabel
            control={
              <Checkbox
                size="small"
                checked={isAllSelected}
                indeterminate={isIndeterminate}
                onChange={toggleAll}
              />
            }
            label="全选本页搜索结果"
          />
        </Box>
        <List
          sx={{
            width: '100%',
            bgcolor: 'background.paper',
            flex: 1,
            overflowY: 'auto',
            pt: 0,
          }}
        >
          {filteredApps.map((app) => {
            const labelId = `checkbox-list-label-${app.path}`
            return (
              <ListItem key={app.path} disablePadding>
                <ListItemButton onClick={() => onToggle(app.path)} dense>
                  <ListItemIcon>
                    <Checkbox
                      edge="start"
                      checked={excludeApps.includes(app.path)}
                      tabIndex={-1}
                      disableRipple
                      slotProps={{ input: { 'aria-labelledby': labelId } }}
                    />
                  </ListItemIcon>
                  <ListItemText
                    id={labelId}
                    primary={app.name}
                    secondary={app.path}
                    sx={{ wordBreak: 'break-all' }}
                  />
                </ListItemButton>
              </ListItem>
            )
          })}
        </List>
      </BaseDialog>,
      document.body,
    )
  },
)
