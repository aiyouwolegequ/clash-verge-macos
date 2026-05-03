import {
  ArrowDownwardRounded,
  ArrowUpwardRounded,
  AppsRounded,
  BarChartRounded,
  CleaningServicesRounded,
  RefreshRounded,
} from '@mui/icons-material'
import {
  Box,
  Chip,
  IconButton,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
  keyframes,
} from '@mui/material'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { clearAppTrafficStats, getAppTrafficStats } from '@/services/cmds'
import parseTraffic from '@/utils/parse-traffic'

const spin = keyframes({
  '0%': { transform: 'rotate(0deg)' },
  '100%': { transform: 'rotate(360deg)' },
})

interface AppTrafficItem {
  process_name: string
  process_path: string
  traffic_mode: string
  upload_bytes: number
  download_bytes: number
}
type Period = 'day' | 'week' | 'month'

const modeColors: Record<string, 'success' | 'error' | 'warning' | 'info'> = {
  直连: 'success',
  拦截: 'error',
  TUN: 'warning',
  代理: 'info',
}

export const AppTrafficStats = () => {
  const [period, setPeriod] = useState<Period>('day')
  const [items, setItems] = useState<AppTrafficItem[]>([])
  const [loading, setLoading] = useState(false)

  const fetchData = useCallback(async () => {
    setLoading(true)
    try {
      const data = await getAppTrafficStats(period)
      setItems(data)
    } catch {
      setItems([])
    } finally {
      setLoading(false)
    }
  }, [period])

  useEffect(() => {
    fetchData()
  }, [fetchData])

  const totalUp = useMemo(
    () => items.reduce((sum, i) => sum + i.upload_bytes, 0),
    [items],
  )
  const totalDown = useMemo(
    () => items.reduce((sum, i) => sum + i.download_bytes, 0),
    [items],
  )

  const displayName = (item: AppTrafficItem) => {
    if (!item.process_path || item.process_path.startsWith('<')) {
      return item.process_name || 'Unknown'
    }
    const parts = item.process_path.split('/')
    const last = parts[parts.length - 1]
    if (last.endsWith('.app') || last.includes('.app/')) {
      const appName = item.process_path.match(/\/([^/]+)\.app/)?.[1]
      return appName || last
    }
    return last || item.process_name || 'Unknown'
  }

  if (items.length === 0 && !loading) {
    return null
  }

  return (
    <Paper
      sx={{ p: 2, mb: 2, display: 'flex', flexDirection: 'column', gap: 1 }}
    >
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <BarChartRounded />
          <Typography variant="subtitle1" sx={{ fontWeight: 500 }}>
            应用流量统计
          </Typography>
        </Box>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <ToggleButtonGroup
            size="small"
            value={period}
            exclusive
            onChange={(_, v) => v && setPeriod(v)}
          >
            <ToggleButton value="day">今日</ToggleButton>
            <ToggleButton value="week">本周</ToggleButton>
            <ToggleButton value="month">本月</ToggleButton>
          </ToggleButtonGroup>
          <IconButton size="small" onClick={fetchData} disabled={loading}>
            <RefreshRounded
              fontSize="small"
              sx={
                loading
                  ? { animation: `${spin} 1s linear infinite` }
                  : undefined
              }
            />
          </IconButton>
          <IconButton
            size="small"
            onClick={async () => {
              await clearAppTrafficStats()
              setItems([])
            }}
          >
            <CleaningServicesRounded fontSize="small" />
          </IconButton>
        </Box>
      </Box>

      <Box sx={{ display: 'flex', gap: 3 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
          <ArrowUpwardRounded fontSize="small" color="error" />
          <Typography variant="caption">{parseTraffic(totalUp)}</Typography>
        </Box>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
          <ArrowDownwardRounded fontSize="small" color="success" />
          <Typography variant="caption">{parseTraffic(totalDown)}</Typography>
        </Box>
      </Box>

      <TableContainer sx={{ maxHeight: 400 }}>
        <Table size="small" stickyHeader>
          <TableHead>
            <TableRow>
              <TableCell>应用</TableCell>
              <TableCell>模式</TableCell>
              <TableCell align="right">上传</TableCell>
              <TableCell align="right">下载</TableCell>
              <TableCell align="right">合计</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {items.map((item) => (
              <TableRow
                key={`${item.process_name}-${item.traffic_mode}-${item.process_path}`}
              >
                <TableCell
                  sx={{
                    maxWidth: 200,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                  }}
                >
                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                    <AppsRounded fontSize="small" color="disabled" />
                    <Typography variant="body2" noWrap>
                      {displayName(item)}
                    </Typography>
                  </Box>
                </TableCell>
                <TableCell>
                  <Chip
                    label={item.traffic_mode}
                    size="small"
                    color={modeColors[item.traffic_mode] || 'default'}
                    variant="outlined"
                  />
                </TableCell>
                <TableCell align="right">
                  <Typography
                    variant="body2"
                    color="error.main"
                    sx={{ fontFamily: 'monospace' }}
                  >
                    {parseTraffic(item.upload_bytes)}
                  </Typography>
                </TableCell>
                <TableCell align="right">
                  <Typography
                    variant="body2"
                    color="success.main"
                    sx={{ fontFamily: 'monospace' }}
                  >
                    {parseTraffic(item.download_bytes)}
                  </Typography>
                </TableCell>
                <TableCell align="right">
                  <Typography variant="body2" sx={{ fontFamily: 'monospace' }}>
                    {parseTraffic(item.upload_bytes + item.download_bytes)}
                  </Typography>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    </Paper>
  )
}

export default AppTrafficStats
