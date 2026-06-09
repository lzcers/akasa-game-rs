type ChangelogEntry = {
  version: string;
  date: string;
  type: "major" | "minor" | "patch";
  title: string;
  description: string;
  changes: {
    type: "new" | "improvement" | "fix" | "story";
    text: string;
  }[];
};

const changelog: ChangelogEntry[] = [
  {
    version: "1.0.0",
    date: "2026-05-28",
    type: "minor",
    title: "初次共鸣",
    description: "",
    changes: [{ type: "new", text: "阿卡夏回响体验第一版" }],
  },
];

const typeLabels = {
  new: "新增记录",
  improvement: "共鸣优化",
  fix: "记录修正",
  story: "剧情回响",
};

export default function ChangelogPage() {
  return (
    <div className="min-h-screen py-24 px-6">
      <div className="max-w-6xl mx-auto">
        <div className="grid gap-8 xl:grid-cols-[minmax(18rem,0.8fr)_minmax(0,1.2fr)] xl:items-start">
          <div className="space-y-5 xl:sticky xl:top-28">
            <div>
              <p className="text-xs tracking-[0.32em] text-accent/80 mb-3">
                记录编年
              </p>
              <h1 className="font-serif text-3xl md:text-4xl text-foreground mb-3">
                记录更新
              </h1>
            </div>

            <div className="game-card p-5">
              <p className="text-sm text-foreground mb-3">当前记录</p>
              <div className="space-y-2 text-sm text-muted-foreground">
                <p>形态：AI 互动小说共鸣原型</p>
                <p>重点：先把世界、角色与序章显影流程落稳</p>
                <p>方向：继续加深记录共鸣、分支因果与终章封存体验</p>
              </div>
            </div>
          </div>

          <div className="space-y-6">
            {changelog.map((entry) => (
              <section key={entry.version} className="p-5 lg:p-6">
                <div className="grid gap-5 lg:grid-cols-[minmax(10rem,0.34fr)_minmax(0,1fr)] lg:gap-8">
                  <div className="lg:border-r lg:border-border/70 lg:pr-6">
                    <p className="text-xs tracking-[0.28em] text-accent/80 mb-3">
                      {entry.date}
                    </p>
                    <p className="font-serif text-2xl text-foreground mb-2">
                      v{entry.version}
                    </p>
                    <span className="inline-flex rounded-full border border-accent/30 bg-accent/8 px-3 py-1 text-xs text-accent">
                      {entry.type === "major"
                        ? "重大更新"
                        : entry.type === "minor"
                          ? "阶段更新"
                          : "修补记录"}
                    </span>
                  </div>

                  <div>
                    <h2 className="font-serif text-2xl text-foreground mb-2">
                      {entry.title}
                    </h2>
                    {entry.description && (
                      <p className="text-foreground/80 leading-7 mb-4">
                        {entry.description}
                      </p>
                    )}
                    <ul className="space-y-3">
                      {entry.changes.map((change, index) => (
                        <li
                          key={`${entry.version}-${index}`}
                          className="game-card p-4 bg-background/35"
                        >
                          <p className="text-sm text-foreground mb-1">
                            {typeLabels[change.type]}
                          </p>
                          <p className="text-sm text-muted-foreground leading-7">
                            {change.text}
                          </p>
                        </li>
                      ))}
                    </ul>
                  </div>
                </div>
              </section>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
