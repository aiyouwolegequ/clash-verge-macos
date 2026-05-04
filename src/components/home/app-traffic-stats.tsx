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
  CircularProgress,
  Dialog,
  DialogContent,
  DialogTitle,
  IconButton,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TableSortLabel,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
  keyframes,
} from '@mui/material'
import { useCallback, useEffect, useMemo, useState } from 'react'

import {
  clearAppTrafficStats,
  getAppTrafficDetail,
  getAppTrafficStats,
} from '@/services/cmds'
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

/** 清理应用名称：去除 .app 后缀、尾部版本号，规范化大小写 */
function cleanAppName(name: string | undefined | null): string | undefined {
  if (!name) return undefined
  let cleaned = name.trim()
  // 去除 .app/.APP 后缀
  cleaned = cleaned.replace(/\.app$/i, '')
  // 去除尾部版本号 " 128.0.6613.138" 或 " 1.2.3"
  cleaned = cleaned.replace(/\s+\d+(?:\.\d+)+\s*$/, '')
  // 去除尾部括号内版本 "(128.0.6613.138)" 或 "(1.2.3)"
  cleaned = cleaned.replace(/\s*\(\d+(?:\.\d+)*\)\s*$/, '')
  // 去除尾部 "v" 前缀版本号 " v2.7.9"
  cleaned = cleaned.replace(/\s+v\d+(?:\.\d+)*\s*$/i, '')
  // 规范化大小写：将 "-" 和 "_" 替换为空格，并首字母大写
  // 但保留已包含大写字母的名称（如 Google Chrome）不被破坏
  if (cleaned && !/[A-Z]/.test(cleaned)) {
    cleaned = cleaned
      .replace(/[-_]/g, ' ')
      .replace(/\b\w/g, (c) => c.toUpperCase())
  }
  return cleaned.trim() || undefined
}

type Order = 'asc' | 'desc'
type SortKey = 'upload_bytes' | 'download_bytes' | 'total_bytes'

export const AppTrafficStats = () => {
  const [period, setPeriod] = useState<Period>('day')
  const [items, setItems] = useState<AppTrafficItem[]>([])
  const [loading, setLoading] = useState(false)
  const [modeFilter, setModeFilter] = useState<string | null>(null)
  const [order, setOrder] = useState<Order>('desc')
  const [orderBy, setOrderBy] = useState<SortKey>('total_bytes')

  const [detailOpen, setDetailOpen] = useState(false)
  const [detailItem, setDetailItem] = useState<AppTrafficItem | null>(null)
  const [detailData, setDetailData] = useState<
    { domain: string; upload_bytes: number; download_bytes: number }[]
  >([])
  const [detailLoading, setDetailLoading] = useState(false)
  const [detailOrder, setDetailOrder] = useState<Order>('desc')
  const [detailOrderBy, setDetailOrderBy] = useState<SortKey>('total_bytes')

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

  const fetchDetail = useCallback(
    async (item: AppTrafficItem) => {
      setDetailLoading(true)
      try {
        const data = await getAppTrafficDetail(
          item.process_path,
          item.traffic_mode,
          period,
        )
        setDetailData(data)
      } catch {
        setDetailData([])
      } finally {
        setDetailLoading(false)
      }
    },
    [period],
  )

  const handleRowClick = (item: AppTrafficItem) => {
    setDetailItem(item)
    setDetailOpen(true)
    fetchDetail(item)
  }

  useEffect(() => {
    fetchData()
  }, [fetchData])

  useEffect(() => {
    if (detailOpen && detailItem) {
      fetchDetail(detailItem)
    }
  }, [detailOpen, detailItem, period, fetchDetail])

  const handleSort = (key: SortKey) => {
    if (orderBy === key) {
      setOrder(order === 'asc' ? 'desc' : 'asc')
    } else {
      setOrderBy(key)
      setOrder('desc')
    }
  }

  const filteredItems = useMemo(() => {
    const result = modeFilter
      ? items.filter((i) => i.traffic_mode === modeFilter)
      : [...items]
    result.sort((a, b) => {
      const cmp =
        orderBy === 'upload_bytes'
          ? a.upload_bytes - b.upload_bytes
          : orderBy === 'download_bytes'
            ? a.download_bytes - b.download_bytes
            : a.upload_bytes +
              a.download_bytes -
              (b.upload_bytes + b.download_bytes)
      return order === 'asc' ? cmp : -cmp
    })
    return result
  }, [items, modeFilter, order, orderBy])

  const totalUp = useMemo(
    () => filteredItems.reduce((sum, i) => sum + i.upload_bytes, 0),
    [filteredItems],
  )
  const totalDown = useMemo(
    () => filteredItems.reduce((sum, i) => sum + i.download_bytes, 0),
    [filteredItems],
  )

  const displayName = (item: AppTrafficItem) => {
    // 无进程路径 → 清理 process_name 后显示
    if (!item.process_path || item.process_path.startsWith('<')) {
      return cleanAppName(item.process_name) || 'Unknown'
    }
    // 优先从 .app bundle 路径提取应用名
    if (item.process_path.includes('.app')) {
      const match = item.process_path.match(/\/([^/]+)\.app/)
      if (match) {
        const cleaned = cleanAppName(match[1])
        if (cleaned) return cleaned
        return match[1]
      }
    }
    // 回退到路径末尾分段
    const last = item.process_path.split('/').pop() || ''
    return cleanAppName(last) || cleanAppName(item.process_name) || 'Unknown'
  }

  const availableModes = useMemo(() => {
    const modes = new Set<string>()
    items.forEach((i) => modes.add(i.traffic_mode))
    return Array.from(modes)
  }, [items])

  const sortedDetailData = useMemo(() => {
    const result = [...detailData]
    result.sort((a, b) => {
      const cmp =
        detailOrderBy === 'upload_bytes'
          ? a.upload_bytes - b.upload_bytes
          : detailOrderBy === 'download_bytes'
            ? a.download_bytes - b.download_bytes
            : a.upload_bytes +
              a.download_bytes -
              (b.upload_bytes + b.download_bytes)
      return detailOrder === 'asc' ? cmp : -cmp
    })
    return result
  }, [detailData, detailOrder, detailOrderBy])

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

      <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap' }}>
        <Chip
          label="全部"
          size="small"
          variant={modeFilter === null ? 'filled' : 'outlined'}
          onClick={() => setModeFilter(null)}
        />
        {availableModes.map((mode) => (
          <Chip
            key={mode}
            label={mode}
            size="small"
            color={modeColors[mode] || 'default'}
            variant={modeFilter === mode ? 'filled' : 'outlined'}
            onClick={() => setModeFilter(mode)}
          />
        ))}
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
              <TableCell align="right">
                <TableSortLabel
                  active={orderBy === 'upload_bytes'}
                  direction={orderBy === 'upload_bytes' ? order : 'desc'}
                  onClick={() => handleSort('upload_bytes')}
                >
                  上传
                </TableSortLabel>
              </TableCell>
              <TableCell align="right">
                <TableSortLabel
                  active={orderBy === 'download_bytes'}
                  direction={orderBy === 'download_bytes' ? order : 'desc'}
                  onClick={() => handleSort('download_bytes')}
                >
                  下载
                </TableSortLabel>
              </TableCell>
              <TableCell align="right">
                <TableSortLabel
                  active={orderBy === 'total_bytes'}
                  direction={orderBy === 'total_bytes' ? order : 'desc'}
                  onClick={() => handleSort('total_bytes')}
                >
                  合计
                </TableSortLabel>
              </TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {filteredItems.map((item) => (
              <TableRow
                key={`${item.process_name}-${item.traffic_mode}-${item.process_path}`}
                onClick={() => handleRowClick(item)}
                sx={{ cursor: 'pointer' }}
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

      <Dialog
        open={detailOpen}
        onClose={() => setDetailOpen(false)}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <AppsRounded fontSize="small" />
            <Typography variant="subtitle1" sx={{ fontWeight: 500 }}>
              {detailItem ? displayName(detailItem) : ''} ·{' '}
              {detailItem?.traffic_mode}
            </Typography>
            {detailLoading && <CircularProgress size={16} />}
          </Box>
        </DialogTitle>
        <DialogContent>
          <TableContainer sx={{ maxHeight: 400 }}>
            <Table size="small" stickyHeader>
              <TableHead>
                <TableRow>
                  <TableCell>域名</TableCell>
                  <TableCell align="right">
                    <TableSortLabel
                      active={detailOrderBy === 'upload_bytes'}
                      direction={
                        detailOrderBy === 'upload_bytes' ? detailOrder : 'desc'
                      }
                      onClick={() => {
                        if (detailOrderBy === 'upload_bytes') {
                          setDetailOrder(detailOrder === 'asc' ? 'desc' : 'asc')
                        } else {
                          setDetailOrderBy('upload_bytes')
                          setDetailOrder('desc')
                        }
                      }}
                    >
                      上传
                    </TableSortLabel>
                  </TableCell>
                  <TableCell align="right">
                    <TableSortLabel
                      active={detailOrderBy === 'download_bytes'}
                      direction={
                        detailOrderBy === 'download_bytes'
                          ? detailOrder
                          : 'desc'
                      }
                      onClick={() => {
                        if (detailOrderBy === 'download_bytes') {
                          setDetailOrder(detailOrder === 'asc' ? 'desc' : 'asc')
                        } else {
                          setDetailOrderBy('download_bytes')
                          setDetailOrder('desc')
                        }
                      }}
                    >
                      下载
                    </TableSortLabel>
                  </TableCell>
                  <TableCell align="right">
                    <TableSortLabel
                      active={detailOrderBy === 'total_bytes'}
                      direction={
                        detailOrderBy === 'total_bytes' ? detailOrder : 'desc'
                      }
                      onClick={() => {
                        if (detailOrderBy === 'total_bytes') {
                          setDetailOrder(detailOrder === 'asc' ? 'desc' : 'asc')
                        } else {
                          setDetailOrderBy('total_bytes')
                          setDetailOrder('desc')
                        }
                      }}
                    >
                      合计
                    </TableSortLabel>
                  </TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {sortedDetailData.map((item) => (
                  <TableRow key={item.domain}>
                    <TableCell
                      sx={{
                        maxWidth: 280,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                      }}
                    >
                      <Typography variant="body2" noWrap>
                        {item.domain}
                      </Typography>
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
                      <Typography
                        variant="body2"
                        sx={{ fontFamily: 'monospace' }}
                      >
                        {parseTraffic(item.upload_bytes + item.download_bytes)}
                      </Typography>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableContainer>
        </DialogContent>
      </Dialog>
    </Paper>
  )
}

export default AppTrafficStats
