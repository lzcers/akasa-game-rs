import { Activity, BarChart3, Clock, KeyRound, RefreshCcw, Users } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

type ApiResponse<T> = {
  success: boolean
  data: T
}

type AnalyticsSummary = {
  rangeHours: number
  generatedAt: string
  totals: {
    events: number
    uniqueUsers: number
    visits: number
    gameSessions: number
  }
  funnel: AnalyticsCount[]
  topSources: AnalyticsSourceCount[]
  topEvents: AnalyticsCount[]
  recentEvents: AnalyticsRecentEvent[]
}

type AnalyticsCount = {
  eventName: string
  count: number
}

type AnalyticsSourceCount = {
  sourceType: string
  source: string
  count: number
}

type AnalyticsRecentEvent = {
  occurredAt: string
  eventName: string
  anonymousUserId: string
  clientSessionId: string
  gameSessionId: string | null
  path: string | null
  source: string | null
  deviceType: string | null
}

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; summary: AnalyticsSummary }
  | { status: 'unauthorized' }
  | { status: 'error'; message: string }

const rangeOptions = [
  { label: '24h', hours: 24 },
  { label: '7d', hours: 24 * 7 },
]

const eventLabels: Record<string, string> = {
  app_opened: '打开应用',
  creation_submitted: '提交创建',
  profile_generate_completed: '设定生成完成',
  generated_profiles_accepted: '接受设定',
  round_reached: '到达回合',
  ending_viewed: '查看结局',
  choice_submitted: '提交选择',
  intuition_preview_used: '使用直觉',
  game_session_create_failed: '创建失败',
  share_clone_session_created: '分享克隆',
}

const sourceTypeLabels: Record<string, string> = {
  utm_source: 'UTM',
  referrer_domain: 'Referrer',
  source_session: 'Share',
  direct: 'Direct',
}

function compactNumber(value: number) {
  return new Intl.NumberFormat('zh-CN', {
    notation: value >= 10000 ? 'compact' : 'standard',
    maximumFractionDigits: 1,
  }).format(value)
}

function eventLabel(eventName: string) {
  return eventLabels[eventName] ?? eventName
}

function formatDate(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }

  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}

function maskId(value: string | null) {
  if (!value) {
    return '-'
  }

  if (value.length <= 14) {
    return value
  }

  return `${value.slice(0, 8)}...${value.slice(-4)}`
}

function emptyText(value: string | null) {
  return value?.trim() || '-'
}

export default function AnalyticsPage() {
  const [rangeHours, setRangeHours] = useState(24)
  const [loadState, setLoadState] = useState<LoadState>({ status: 'loading' })
  const [refreshKey, setRefreshKey] = useState(0)

  useEffect(() => {
    const controller = new AbortController()

    async function loadSummary() {
      setLoadState({ status: 'loading' })
      try {
        const response = await fetch(`/internal/analytics/data?rangeHours=${rangeHours}`, {
          credentials: 'same-origin',
          signal: controller.signal,
        })

        if (response.status === 401) {
          setLoadState({ status: 'unauthorized' })
          return
        }

        if (!response.ok) {
          setLoadState({ status: 'error', message: `数据源返回 ${response.status}` })
          return
        }

        if (!response.headers.get('content-type')?.includes('application/json')) {
          setLoadState({ status: 'error', message: '数据源不可用' })
          return
        }

        const body = await response.json() as ApiResponse<AnalyticsSummary>
        if (!body.success) {
          setLoadState({ status: 'error', message: '数据源返回失败状态' })
          return
        }

        setLoadState({ status: 'ready', summary: body.data })
      } catch (error) {
        if (controller.signal.aborted) {
          return
        }

        setLoadState({
          status: 'error',
          message: error instanceof Error ? error.message : '无法读取数据',
        })
      }
    }

    void loadSummary()
    const timer = window.setInterval(() => setRefreshKey((key) => key + 1), 30_000)

    return () => {
      controller.abort()
      window.clearInterval(timer)
    }
  }, [rangeHours, refreshKey])

  const summary = loadState.status === 'ready' ? loadState.summary : null
  const firstFunnelCount = useMemo(() => {
    if (!summary) {
      return 0
    }
    return summary.funnel[0]?.count ?? 0
  }, [summary])

  return (
    <div className="min-h-screen px-4 py-20 sm:px-6 lg:py-24">
      <div className="mx-auto max-w-6xl">
        <div className="mb-7 flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <p className="mb-3 text-xs tracking-[0.32em] text-accent/80">
              INTERNAL ANALYTICS
            </p>
            <h1 className="font-serif text-3xl text-foreground md:text-4xl">
              指标看板
            </h1>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <div className="inline-flex rounded-lg border border-border bg-card/70 p-1">
              {rangeOptions.map((option) => (
                <button
                  key={option.hours}
                  type="button"
                  onClick={() => setRangeHours(option.hours)}
                  className={`min-h-9 min-w-14 rounded-md px-3 text-sm transition-colors ${rangeHours === option.hours
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:bg-secondary hover:text-foreground'
                    }`}
                >
                  {option.label}
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={() => setRefreshKey((key) => key + 1)}
              className="game-btn-secondary inline-flex min-h-10 items-center gap-2 px-4 text-sm"
            >
              <RefreshCcw className="h-4 w-4" />
              刷新
            </button>
          </div>
        </div>

        <StatusPanel loadState={loadState} />

        {summary && (
          <div className="space-y-6">
            <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
              <MetricTile label="事件数" value={summary.totals.events} icon={<Activity className="h-5 w-5" />} />
              <MetricTile label="独立用户" value={summary.totals.uniqueUsers} icon={<Users className="h-5 w-5" />} />
              <MetricTile label="访问会话" value={summary.totals.visits} icon={<Clock className="h-5 w-5" />} />
              <MetricTile label="游戏会话" value={summary.totals.gameSessions} icon={<BarChart3 className="h-5 w-5" />} />
            </section>

            <section className="grid gap-6 xl:grid-cols-[minmax(0,1.15fr)_minmax(20rem,0.85fr)]">
              <div className="game-card p-5 lg:p-6">
                <SectionHeader title="核心漏斗" detail={`${summary.rangeHours} 小时窗口`} />
                <div className="space-y-4">
                  {summary.funnel.map((item) => {
                    const rate = firstFunnelCount > 0 ? Math.round((item.count / firstFunnelCount) * 100) : 0
                    return (
                      <div key={item.eventName}>
                        <div className="mb-2 flex items-center justify-between gap-4 text-sm">
                          <span className="text-foreground">{eventLabel(item.eventName)}</span>
                          <span className="font-mono text-muted-foreground">
                            {compactNumber(item.count)} / {rate}%
                          </span>
                        </div>
                        <div className="h-2 overflow-hidden rounded-full bg-secondary">
                          <div
                            className="h-full rounded-full bg-accent"
                            style={{ width: `${Math.min(rate, 100)}%` }}
                          />
                        </div>
                      </div>
                    )
                  })}
                </div>
              </div>

              <div className="game-card p-5 lg:p-6">
                <SectionHeader title="热门事件" detail="按事件量排序" />
                <div className="space-y-3">
                  {summary.topEvents.length === 0 && <EmptyRow text="暂无事件" />}
                  {summary.topEvents.map((item) => (
                    <div key={item.eventName} className="flex items-center justify-between gap-4 rounded-md bg-background/35 px-3 py-2">
                      <span className="truncate text-sm text-foreground">{eventLabel(item.eventName)}</span>
                      <span className="font-mono text-sm text-accent">{compactNumber(item.count)}</span>
                    </div>
                  ))}
                </div>
              </div>
            </section>

            <section className="grid gap-6 xl:grid-cols-[minmax(20rem,0.85fr)_minmax(0,1.15fr)]">
              <div className="game-card p-5 lg:p-6">
                <SectionHeader title="来源" detail="Top 10" />
                <div className="overflow-x-auto">
                  <table className="w-full min-w-[24rem] text-left text-sm">
                    <thead className="text-xs text-muted-foreground">
                      <tr className="border-b border-border">
                        <th className="pb-3 font-normal">类型</th>
                        <th className="pb-3 font-normal">来源</th>
                        <th className="pb-3 text-right font-normal">事件</th>
                      </tr>
                    </thead>
                    <tbody>
                      {summary.topSources.length === 0 && (
                        <tr>
                          <td colSpan={3}>
                            <EmptyRow text="暂无来源" />
                          </td>
                        </tr>
                      )}
                      {summary.topSources.map((source) => (
                        <tr key={`${source.sourceType}:${source.source}`} className="border-b border-border/50 last:border-0">
                          <td className="py-3 text-muted-foreground">{sourceTypeLabels[source.sourceType] ?? source.sourceType}</td>
                          <td className="max-w-48 truncate py-3 text-foreground">{source.source}</td>
                          <td className="py-3 text-right font-mono text-accent">{compactNumber(source.count)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>

              <div className="game-card p-5 lg:p-6">
                <SectionHeader title="最近事件" detail={`更新于 ${formatDate(summary.generatedAt)}`} />
                <div className="overflow-x-auto">
                  <table className="w-full min-w-[44rem] text-left text-sm">
                    <thead className="text-xs text-muted-foreground">
                      <tr className="border-b border-border">
                        <th className="pb-3 font-normal">时间</th>
                        <th className="pb-3 font-normal">事件</th>
                        <th className="pb-3 font-normal">用户</th>
                        <th className="pb-3 font-normal">会话</th>
                        <th className="pb-3 font-normal">来源</th>
                        <th className="pb-3 font-normal">路径</th>
                      </tr>
                    </thead>
                    <tbody>
                      {summary.recentEvents.length === 0 && (
                        <tr>
                          <td colSpan={6}>
                            <EmptyRow text="暂无最近事件" />
                          </td>
                        </tr>
                      )}
                      {summary.recentEvents.map((event, index) => (
                        <tr key={`${event.occurredAt}:${event.eventName}:${index}`} className="border-b border-border/50 last:border-0">
                          <td className="whitespace-nowrap py-3 text-muted-foreground">{formatDate(event.occurredAt)}</td>
                          <td className="py-3 text-foreground">{eventLabel(event.eventName)}</td>
                          <td className="py-3 font-mono text-xs text-muted-foreground">{maskId(event.anonymousUserId)}</td>
                          <td className="py-3 font-mono text-xs text-muted-foreground">{maskId(event.gameSessionId ?? event.clientSessionId)}</td>
                          <td className="max-w-36 truncate py-3 text-muted-foreground">{emptyText(event.source)}</td>
                          <td className="max-w-48 truncate py-3 text-muted-foreground">{emptyText(event.path)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            </section>
          </div>
        )}
      </div>
    </div>
  )
}

function StatusPanel({ loadState }: { loadState: LoadState }) {
  if (loadState.status === 'ready') {
    return (
      <div className="mb-6 flex items-center gap-3 rounded-lg border border-accent/30 bg-accent/8 px-4 py-3 text-sm text-accent">
        <KeyRound className="h-4 w-4 shrink-0" />
        已连接 analytics 数据源
      </div>
    )
  }

  if (loadState.status === 'loading') {
    return (
      <div className="mb-6 flex items-center gap-3 rounded-lg border border-border bg-card/60 px-4 py-3 text-sm text-muted-foreground">
        <KeyRound className="h-4 w-4 shrink-0" />
        正在验证访问权限...
      </div>
    )
  }

  if (loadState.status === 'unauthorized') {
    return (
      <div className="mb-6 rounded-lg border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
        鉴权失败或已过期，请重新登录。
      </div>
    )
  }

  return (
    <div className="mb-6 rounded-lg border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      无法读取数据：{loadState.message}
    </div>
  )
}

function MetricTile({ label, value, icon }: { label: string; value: number; icon: React.ReactNode }) {
  return (
    <div className="game-card p-5">
      <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-accent/12 text-accent">
        {icon}
      </div>
      <p className="mb-1 text-sm text-muted-foreground">{label}</p>
      <p className="font-mono text-3xl text-foreground">{compactNumber(value)}</p>
    </div>
  )
}

function SectionHeader({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="mb-5 flex items-center justify-between gap-4">
      <h2 className="font-serif text-xl text-foreground">{title}</h2>
      <span className="text-xs text-muted-foreground">{detail}</span>
    </div>
  )
}

function EmptyRow({ text }: { text: string }) {
  return (
    <div className="rounded-md border border-dashed border-border px-3 py-5 text-center text-sm text-muted-foreground">
      {text}
    </div>
  )
}
